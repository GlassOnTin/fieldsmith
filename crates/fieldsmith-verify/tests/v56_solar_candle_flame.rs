//! V56 — Solar-Focused Optical Lens Candle Ignition, Buoyant Diffusion Flame & Smoke Plume
//! (`solar_candle_ignition_v1`), Transient Keyframe Playback, and Milestone Recap Verification.
//!
//! Verifies:
//! 1. Thick-lens BK7 optical glass refraction, focal spot waist, and solar concentration physics:
//!    - Larger aperture `D_lens` increases optical concentration `C_opt`, increases peak solar
//!      caustic flux `q_focus`, and strictly shortens auto-ignition delay `t_ign`.
//!    - Axial lens defocus `|z_lens - f|` expands the caustic spot radius `w(Δz)` and lengthens
//!      auto-ignition delay `t_ign`.
//! 2. Monolithic 144-DOF Newton-Raphson convergence (`24` axial stations × `6` coupled fields:
//!    `u_z`, `Z_mix`, `T_gas`, `Y_OH*`, `f_v`, `q_rad`), residual reduction `> 10⁸×`, and
//!    directional finite-difference Jacobian verification (`< 1e-5` relative error).
//! 3. Sparse Adjoint (`Jᵀ λ = -(∂Φ/∂W)ᵀ`) vs Sparse Tangent (`J v = -∂R/∂p`) sensitivity
//!    verification across all 5 engineering objectives (`t_ign`, `T_max`, `P_rad`, `ṁ_smoke`, `u_z,max`)
//!    and 6 design parameters (`< 1e-6` relative discrepancy).
//! 4. Exact mass, enthalpy, and soot/smoke conservation audits (`0` net imbalance, `< 1e-6`
//!    relative error).
//! 5. Time-dependent transient progression (`40` frames over `t ∈ [0, 6+ s]`) through all 5
//!    physical regimes (Solar Caustic Heating → Paraffin Pool Melting at 331 K → Pyrolysis Vapor
//!    Plume at 620 K → Flash Auto-Ignition at 670 K → Buoyant Flickering Diffusion Flame & Smoke
//!    Plume) and 5 automated milestone recap cards (`SimulationRecapEvent`).
//! 6. 3D `.fsm` parametric CAD & thermo-mechanical FEA twin synthesis and validation, plus MCP
//!    `list_coupled_systems`, `solve_coupled_system`, and `optimise_coupled_system` round-trip.

use fieldsmith_doc::fsm;
use fieldsmith_mcp::Server;
use fieldsmith_solve::candle_flame::SolarCandleIgnitionSystem;
use fieldsmith_solve::coupled_system::{
    CoupledEngineeringSystem, EngineConfig, MonolithicSystemEngine,
};
use fieldsmith_verify::{Check, Report};
use serde_json::json;

#[test]
fn v56_solar_candle_flame() {
    let mut r = Report::new(
        "V56",
        "Solar-Focused Optical Lens Candle Ignition, Buoyant Diffusion Flame, Smoke Plume & Transient Recap (`solar_candle_ignition_v1`)",
    );

    let sys = SolarCandleIgnitionSystem::default();
    let cfg = EngineConfig::default();
    let p_nom = sys.nominal_design_vector();
    let theta_nom = sys.nominal_epistemic_vector();

    // -------------------------------------------------------------------------
    // 1. Monolithic 144-DOF Newton Solve, Jacobian FD, and Adjoint vs Tangent
    // -------------------------------------------------------------------------
    let analysis = MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p_nom, &theta_nom)
        .expect("solar_candle_ignition_v1 monolithic solve should converge");

    r.check(Check::absolute(
        "Monolithic state vector DOFs (`24 stations × 6 fields`)",
        144.0,
        analysis.num_dofs as f64,
        "DOF",
        0.0,
    ));
    r.check(Check::absolute(
        "Monolithic Newton converged residual `||R(W*)||_2`",
        0.0,
        analysis.final_residual_l2,
        "L2",
        1e-9,
    ));
    r.check(Check::absolute(
        "Directional FD Jacobian verification `||J·v - FD(v)|| / ||FD(v)||`",
        0.0,
        analysis.directional_fd_jacobian_rel_err,
        "rel",
        1e-5,
    ));
    r.check(Check::absolute(
        "Sparse Adjoint vs Sparse Tangent `max |dΦ/dp_adj - dΦ/dp_tan|`",
        0.0,
        analysis.adjoint_vs_tangent_max_rel_err,
        "rel",
        1e-6,
    ));

    // -------------------------------------------------------------------------
    // 2. Optical Caustic & Auto-Ignition Physics Checks
    // -------------------------------------------------------------------------
    let w_star = &analysis.state;
    let vis_nom = sys.extract_visual_fields(w_star, &p_nom, &theta_nom);

    // Thick-lens focal length for R = 68 mm BK7 (n = 1.5168, t_c = 11 mm) is ~68 mm
    r.check(Check::relative(
        "Thick-lens BK7 focal length `f` (`R = 68 mm`, `n = 1.5168`)",
        67.2,
        vis_nom.focal_length_mm,
        "mm",
        0.05,
    ));

    // Larger lens aperture (88 mm vs 48 mm) at focal length boosts C_opt and shortens t_ign
    let mut p_small_lens = p_nom.clone();
    p_small_lens[0] = 48.0;
    p_small_lens[2] = vis_nom.focal_length_mm;
    let mut p_large_lens = p_nom.clone();
    p_large_lens[0] = 88.0;
    p_large_lens[2] = vis_nom.focal_length_mm;

    let res_small =
        MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p_small_lens, &theta_nom).unwrap();
    let res_large =
        MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p_large_lens, &theta_nom).unwrap();
    let vis_small = sys.extract_visual_fields(&res_small.state, &p_small_lens, &theta_nom);
    let vis_large = sys.extract_visual_fields(&res_large.state, &p_large_lens, &theta_nom);

    r.check(Check::absolute(
        "Larger lens aperture (`88 mm` vs `48 mm`) increases solar concentration `> 1.5×`",
        1.0,
        if vis_large.optical_concentration > vis_small.optical_concentration * 1.5
            && vis_large.peak_solar_flux_kw_m2 > vis_small.peak_solar_flux_kw_m2 * 1.5
        {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));
    r.check(Check::absolute(
        "Larger lens aperture (`88 mm` vs `48 mm`) shortens auto-ignition delay `t_ign`",
        1.0,
        if vis_large.ignition_delay_s < vis_small.ignition_delay_s {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));

    // Defocus check: moving z_lens away from focal length f expands spot radius w(Δz) and increases t_ign
    let mut p_focus = p_nom.clone();
    p_focus[2] = vis_nom.focal_length_mm;
    let mut p_defocus = p_nom.clone();
    p_defocus[2] = vis_nom.focal_length_mm + 22.0;

    let res_focus =
        MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p_focus, &theta_nom).unwrap();
    let res_defocus =
        MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p_defocus, &theta_nom).unwrap();
    let vis_focus = sys.extract_visual_fields(&res_focus.state, &p_focus, &theta_nom);
    let vis_defocus = sys.extract_visual_fields(&res_defocus.state, &p_defocus, &theta_nom);

    r.check(Check::absolute(
        "Axial defocus (`|z_lens - f| = 22 mm`) expands spot waist `> 2×` and delays ignition `> 1.4×`",
        1.0,
        if vis_defocus.spot_radius_mm > vis_focus.spot_radius_mm * 2.0
            && vis_defocus.ignition_delay_s > vis_focus.ignition_delay_s * 1.4
        {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));

    // -------------------------------------------------------------------------
    // 3. Combustion Chemistry, Buoyant Plume & Smoke Point Physics
    // -------------------------------------------------------------------------
    let mut p_long_wick = p_nom.clone();
    p_long_wick[4] = 12.5;
    let res_long_wick =
        MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p_long_wick, &theta_nom).unwrap();

    let vis_long_wick = sys.extract_visual_fields(&res_long_wick.state, &p_long_wick, &theta_nom);
    let smoke_nom = analysis.objectives[3];
    let smoke_long = res_long_wick.objectives[3];
    let rad_nom = analysis.objectives[2];
    let rad_long = res_long_wick.objectives[2];

    r.check(Check::absolute(
        "Longer wick (`12.5 mm` vs `7.5 mm`) increases pyrolysis, radiant power, and escaping smoke",
        1.0,
        if smoke_long > smoke_nom * 1.15
            && rad_long > rad_nom * 1.10
            && analysis.objectives[1] > 1800.0
            && analysis.objectives[4] > 0.45
        {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));

    r.check(Check::absolute(
        "Thermophoretic soot deposition on overhead lens (`δ_soot > 15 nm`) grows with wick smoke & dims `τ_lens`",
        1.0,
        if vis_nom.final_lens_soot_nm > 15.0
            && vis_long_wick.final_lens_soot_nm > vis_nom.final_lens_soot_nm * 1.15
            && vis_long_wick.final_lens_transmittance_pct < vis_nom.final_lens_transmittance_pct
            && vis_nom.final_lens_transmittance_pct < 90.0
            && vis_nom.lens_soot_radial_nm.first().copied().unwrap_or(0.0)
                > vis_nom.lens_soot_radial_nm.last().copied().unwrap_or(1e9)
        {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));

    // Verify blue OH* chemiluminescence peaks near the flame base while soot f_v peaks higher in the yellow luminous zone
    let (oh_peak_idx, oh_peak_val) = vis_nom
        .y_oh_ppm
        .iter()
        .copied()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .unwrap();
    let (soot_peak_idx, soot_peak_val) = vis_nom
        .fv_soot_ppm
        .iter()
        .copied()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .unwrap();
    let z_oh_peak = vis_nom.z_mm[oh_peak_idx];
    let z_soot_peak = vis_nom.z_mm[soot_peak_idx];

    r.check(Check::absolute(
        "Axial flame stratification: blue `OH*` peaks below yellow soot core (`z_OH* < z_soot`)",
        1.0,
        if z_oh_peak < z_soot_peak && oh_peak_val > 100.0 && soot_peak_val > 1.0 {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));

    // -------------------------------------------------------------------------
    // 4. Conservation Audits (Reactant Mass, Enthalpy, and Soot/Smoke Balance)
    // -------------------------------------------------------------------------
    let max_audit_err = analysis
        .conservation_audits
        .iter()
        .map(|a| a.relative_error.max(a.net_imbalance.abs()))
        .fold(0.0_f64, f64::max);

    r.check(Check::absolute(
        "3 conservation balances (Reactant Mass, Combustion+Solar Enthalpy, Soot/Smoke) imbalance",
        0.0,
        max_audit_err,
        "rel",
        1e-9,
    ));

    // -------------------------------------------------------------------------
    // 5. Time-Dependent Transient Playback (40 Frames) & Milestone Recap Cards
    // -------------------------------------------------------------------------
    let frames = &vis_nom.frames;
    let recaps = &vis_nom.recap_events;
    let f_first = frames.first().unwrap();
    let f_last = frames.last().unwrap();
    let has_pre_ignition = frames.iter().any(|f| !f.ignited && f.wick_temp_k < 670.0);
    let has_post_ignition = frames
        .iter()
        .any(|f| f.ignited && f.flame_height_mm > 25.0 && f.radiant_power_w > 5.0);

    r.check(Check::absolute(
        "Transient simulation keyframe count (`t ∈ [0, 6+ s]`)",
        40.0,
        frames.len() as f64,
        "frames",
        0.0,
    ));
    r.check(Check::absolute(
        "Transient progression spans pre-ignition heating/melting/pyrolysis and post-ignition buoyant flame",
        1.0,
        if !f_first.ignited
            && f_last_valid(f_first, f_last)
            && has_pre_ignition
            && has_post_ignition
        {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));

    let recaps_monotonic = recaps.len() == 5
        && recaps.windows(2).all(|w| {
            w[0].time_s <= w[1].time_s
                && w[0].frame_index <= w[1].frame_index
                && w[1].frame_index < frames.len()
        });

    r.check(Check::absolute(
        "5 `SimulationRecapEvent` milestone cards index monotonically into transient frames",
        1.0,
        if recaps_monotonic { 1.0 } else { 0.0 },
        "flag",
        0.0,
    ));

    // -------------------------------------------------------------------------
    // 6. 3D Parametric .fsm CAD Twin & MCP Co-Pilot Round-Trip
    // -------------------------------------------------------------------------
    let fsm_text = sys
        .build_3d_fsm(&p_nom)
        .expect("3D .fsm twin should be generated");
    let doc = fsm::parse(&fsm_text).expect("3D .fsm twin should parse cleanly");
    let problems = doc.validate();

    r.check(Check::absolute(
        "3D parametric `.fsm` CAD + thermo-mechanical FEA twin validation problems",
        0.0,
        problems.len() as f64,
        "count",
        0.0,
    ));

    let mut mcp = Server::new();
    let solve_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "solve_coupled_system",
            "arguments": {
                "system_id": "solar_candle_ignition_v1",
                "design_p": [55.0, 68.0, 92.0, 780.0, 12.0, 0.21],
                "sync_cad": true
            }
        }
    });
    let solve_resp_str = mcp
        .handle(&solve_req.to_string())
        .expect("MCP solve_coupled_system response");
    let solve_resp: serde_json::Value = serde_json::from_str(&solve_resp_str).unwrap();
    let solve_text = solve_resp["result"]["content"][0]["text"].as_str().unwrap();
    let solve_payload: serde_json::Value = serde_json::from_str(solve_text).unwrap();

    let opt_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "optimise_coupled_system",
            "arguments": {
                "system_id": "solar_candle_ignition_v1",
                "steps": 3,
                "sync_cad": true
            }
        }
    });
    let opt_resp_str = mcp
        .handle(&opt_req.to_string())
        .expect("MCP optimise_coupled_system response");
    let opt_resp: serde_json::Value = serde_json::from_str(&opt_resp_str).unwrap();
    let opt_text = opt_resp["result"]["content"][0]["text"].as_str().unwrap();
    let opt_payload: serde_json::Value = serde_json::from_str(opt_text).unwrap();

    let t_ign_before = solve_payload["analysis"]["objectives"][0].as_f64().unwrap();
    let t_ign_after = opt_payload["final_objectives"][0]
        .as_f64()
        .unwrap();
    let has_visual_frames = solve_payload["candle_flame_visual"]["frames"]
        .as_array()
        .map(|a| a.len() == 40)
        .unwrap_or(false);

    r.check(Check::absolute(
        "MCP `solve_coupled_system` + 3-step `optimise_coupled_system` reduces `t_ign` and syncs 3D CAD",
        1.0,
        if has_visual_frames
            && solve_payload["synced_to_cad_doc"].as_bool() == Some(true)
            && opt_payload["synced_to_cad_doc"].as_bool() == Some(true)
            && t_ign_after < t_ign_before
        {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));

    r.section(
        "Solar Caustic Optics, Combustion Chemistry & Transient Recap Summary",
        format!(
            "`solar_candle_ignition_v1` couples thick-lens BK7 borosilicate refraction (`f = {:.2} mm`, \
             `C_opt = {:.0}×`, `q_focus = {:.0} kW/m²`), paraffin (`C₂₅H₅₂`) solid-to-liquid melting (`331 K`), \
             capillary wick pyrolysis (`620 K`), flash auto-ignition (`670 K` at `t_ign = {:.2} s`), \
             Burke–Schumann mixture-fraction diffusion flame chemistry (`Z_st = {:.4}`), excited `OH*/CH*` \
             blue chemiluminescence (`{:.1} ppm` peak at `z = {:.1} mm`), Moss–Brookes soot inception and \
             Nagle–Strickland-Constable thermal oxidation (`{:.2} ppm` peak soot at `z = {:.1} mm`), \
             and Boussinesq buoyant convection (`u_z,max = {:.2} m/s`, `ṁ_smoke = {:.2} μg/s`).\n\n\
             - **40 transient keyframes** (`t = {:.2} s → {:.2} s`) and **5 milestone recap cards** \
             enable interactive timeline scrubbing, instant regime jumping, and real-time `.webm` video synthesis.\n\
             - **MCP Sparse Adjoint optimisation**: 3 steps on the defocused scenario reduced ignition delay \
             from `{:.2} s` to `{:.2} s` while updating the 9-body 3D `.fsm` CAD & thermo-mechanical FEA twin.",
            vis_nom.focal_length_mm,
            vis_nom.optical_concentration,
            vis_nom.peak_solar_flux_kw_m2,
            vis_nom.ignition_delay_s,
            vis_nom.stoichiometric_z,
            oh_peak_val,
            z_oh_peak,
            soot_peak_val,
            z_soot_peak,
            analysis.objectives[4],
            analysis.objectives[3],
            f_first.time_s,
            f_last.time_s,
            t_ign_before,
            t_ign_after
        ),
    );

    r.finish();
}

fn f_last_valid(
    first: &fieldsmith_solve::candle_flame::CandleTransientFrame,
    last: &fieldsmith_solve::candle_flame::CandleTransientFrame,
) -> bool {
    first.wick_temp_k < 330.0
        && last.ignited
        && last.wick_temp_k > 670.0
        && last.peak_flame_temp_k > 1750.0
        && last.r_flame_mm.len() == 24
}
