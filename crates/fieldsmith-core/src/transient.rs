//! 3D P2 transient thermo-elastic conduction (`ρ c_p ∂T/∂t = ∇·(k ∇T)`) and batched
//! multi-RHS Duhamel–Neumann thermal-shock stress (`K_el u(t_m) = f_th(T(t_m) - T_ref)`).

use crate::{
    case_convections_all_with_regions, case_heat_fluxes_all_with_regions,
    case_temperatures_with_regions, compute_nodal_temperature_gradient, elem_stress_thermal_multi,
    prepare, structural_doc, CaseResult, Clock, Facet, Material, Prepared, Provenance,
    RegionLookup, Resolution, SetupError,
};
use fieldsmith_doc::Document;
use fieldsmith_solve::{
    compliance, min_compressive, nodal_stress_thermal_multi, solve_conduction_spatial,
    solve_multi_rhs, solve_transient_conduction_spatial, thermal_load_multi, von_mises, P2Mesh,
    SolverOptions, Stress, ThermalRefData,
};
use serde::Serialize;

/// Default transient step count when `solve t_end=...` omits `steps=`.
pub const DEFAULT_TRANSIENT_STEPS: usize = 20;

/// Per-frame scalar summary along a transient thermo-elastic trajectory.
#[derive(Clone, Debug, Serialize)]
pub struct TransientStepSummary {
    pub step: usize,
    pub time_s: f64,
    pub min_temp_c: f64,
    pub max_temp_c: f64,
    pub mean_temp_c: f64,
    pub max_vm_elem: f64,
    pub max_von_mises: f64,
    pub max_displacement: f64,
    pub conduction_iterations: usize,
    pub elastic_iterations: usize,
}

/// Complete transient thermo-elastic trajectory and thermal-shock summary for one thermal case.
#[derive(Clone, Debug, Serialize)]
pub struct TransientCaseReport {
    pub case_name: String,
    pub scheme: String,
    pub t_init_c: f64,
    pub t_ref_c: f64,
    pub t_end_s: f64,
    pub dt_s: f64,
    pub steps_count: usize,
    pub diffusivity_mm2_s: f64,
    pub peak_vm_elem: f64,
    pub peak_von_mises: f64,
    pub peak_displacement: f64,
    pub t_at_peak_vm_s: f64,
    pub step_at_peak_vm: usize,
    pub min_temp_overall_c: f64,
    pub max_temp_overall_c: f64,
    pub final_min_temp_c: f64,
    pub final_max_temp_c: f64,
    pub final_vm_elem: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steady_limit_vm_elem: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shock_amplification: Option<f64>,
    pub timeline: Vec<TransientStepSummary>,
    /// Nodal temperature (`°C`, `nn` floats) for each frame `0..=steps_count`.
    #[serde(skip)]
    pub frames_temperature: Vec<Vec<f64>>,
    /// Nodal displacement (`mm`, `3 * nn` floats) for each frame `0..=steps_count`.
    #[serde(skip)]
    pub frames_u: Vec<Vec<f64>>,
    /// Nodal von Mises (`MPa`, `nn` floats) for each frame `0..=steps_count`.
    #[serde(skip)]
    pub frames_vm_nodal: Vec<Vec<f64>>,
    /// Element-centroid von Mises (`MPa`, `ne` floats) for each frame `0..=steps_count`.
    #[serde(skip)]
    pub frames_vm_elem: Vec<Vec<f64>>,
}

/// Solve one transient thermo-elastic case on an already-prepared P2 mesh, returning both
/// the worst-case peak-shock [`CaseResult`] (for `Analysis.cases`, envelopes, and TPMS
/// lattice grading) and the full [`TransientCaseReport`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_transient_thermal_case_multi_with_regions(
    doc: &Document,
    mesh: &P2Mesh,
    young: &[f64],
    poisson: f64,
    alpha_elem: &[f64],
    fixed: &[u8],
    facets: &[Facet],
    name: String,
    h: f64,
    k_phys_elem: &[f64],
    c_vol_elem: &[f64],
    diffusivity_mm2_s: f64,
    t_init: f64,
    t_end: f64,
    steps: usize,
    region_lookup: Option<&RegionLookup<'_>>,
) -> Result<(CaseResult, TransientCaseReport), SetupError> {
    if !(t_end.is_finite() && t_end > 0.0) {
        return Err(SetupError(format!(
            "transient solve: t_end must be > 0 (got {t_end})"
        )));
    }
    if !(1..=500).contains(&steps) {
        return Err(SetupError(format!(
            "transient solve: steps must be in 1..=500 (got {steps})"
        )));
    }
    if !t_init.is_finite() {
        return Err(SetupError(format!(
            "transient solve: t_init must be finite (got {t_init})"
        )));
    }

    let d = case_temperatures_with_regions(doc, mesh, facets, &name, h, region_lookup)?;
    let (robin, spatial_robin) =
        case_convections_all_with_regions(doc, mesh, facets, &name, h, region_lookup)?;
    let (flux, spatial_flux) =
        case_heat_fluxes_all_with_regions(doc, mesh, facets, &name, h, region_lookup)?;

    let frames = solve_transient_conduction_spatial(
        mesh,
        &d,
        k_phys_elem,
        c_vol_elem,
        t_init,
        t_end,
        steps,
        &robin,
        &spatial_robin,
        &flux,
        &spatial_flux,
        SolverOptions::default(),
    )
    .map_err(SetupError)?;

    // Attempt the steady-state limit (`t -> inf`) on the exact same physical `k_phys_elem`
    // so `steady_limit_vm_elem` and `shock_amplification` can be compared directly. On a
    // purely Neumann flux-heated part with no Dirichlet/Robin anchor, steady conduction has
    // no finite limit (`Ok` -> `Some`, `Err` -> `None`).
    let steady_temp = solve_conduction_spatial(
        mesh,
        &d,
        k_phys_elem,
        &robin,
        &spatial_robin,
        &flux,
        &spatial_flux,
        SolverOptions::default(),
    )
    .ok()
    .map(|(t, _)| t);

    let nn = mesh.nodes.len();
    let ne = mesh.tets.len();
    let t_ref = doc.solve.t_ref.unwrap_or(20.0);
    let has_elastic = fixed.iter().any(|&m| m != 0) && alpha_elem.iter().any(|&a| a.abs() >= 1e-18);

    let mut timeline = Vec::with_capacity(frames.len());
    let mut frames_temperature = Vec::with_capacity(frames.len());
    let mut frames_u = Vec::with_capacity(frames.len());
    let mut frames_vm_nodal = Vec::with_capacity(frames.len());
    let mut frames_vm_elem = Vec::with_capacity(frames.len());

    let mut min_temp_overall = f64::INFINITY;
    let mut max_temp_overall = f64::NEG_INFINITY;

    if !has_elastic {
        for fr in &frames {
            let min_t = fr.temperature.iter().copied().fold(f64::INFINITY, f64::min);
            let max_t = fr
                .temperature
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            let mean_t = fr.temperature.iter().sum::<f64>() / (nn.max(1) as f64);
            min_temp_overall = min_temp_overall.min(min_t);
            max_temp_overall = max_temp_overall.max(max_t);
            timeline.push(TransientStepSummary {
                step: fr.step,
                time_s: fr.time_s,
                min_temp_c: min_t,
                max_temp_c: max_t,
                mean_temp_c: mean_t,
                max_vm_elem: 0.0,
                max_von_mises: 0.0,
                max_displacement: 0.0,
                conduction_iterations: fr.report.iterations,
                elastic_iterations: 0,
            });
            frames_temperature.push(fr.temperature.clone());
            frames_u.push(vec![0.0; nn * 3]);
            frames_vm_nodal.push(vec![0.0; nn]);
            frames_vm_elem.push(vec![0.0; ne]);
        }
        let last = frames.last().expect("at least 2 frames");
        let grad_t = compute_nodal_temperature_gradient(mesh, &last.temperature);
        let case_res = CaseResult {
            name: name.clone(),
            max_von_mises: 0.0,
            max_vm_elem: 0.0,
            max_displacement: 0.0,
            compliance: 0.0,
            compliance_upper: None,
            u: vec![0.0; nn * 3],
            stress: vec![Stress::default(); nn],
            von_mises: vec![0.0; nn],
            sigma_min_elem: vec![0.0; ne],
            stress_elem: vec![Stress::default(); ne],
            plastic_strain_elem: vec![0.0; ne],
            grad_t_nodal: Some(grad_t),
            peel_nodal: Vec::new(),
            peel_elem: vec![0.0; ne],
            max_layer_peel: 0.0,
            contact_pressure: Vec::new(),
            vm_elem: vec![0.0; ne],
            solve: last.report.clone(),
            load_reports: Vec::new(),
            contact_reports: Vec::new(),
            temperature: Some(last.temperature.clone()),
            conduction: Some(last.report.clone()),
        };
        let last_sum = timeline.last().expect("at least 2 summaries");
        let report = TransientCaseReport {
            case_name: name,
            scheme: if steps >= 2 { "bdf2" } else { "bdf1" }.into(),
            t_init_c: t_init,
            t_ref_c: t_ref,
            t_end_s: t_end,
            dt_s: t_end / (steps as f64),
            steps_count: steps,
            diffusivity_mm2_s,
            peak_vm_elem: 0.0,
            peak_von_mises: 0.0,
            peak_displacement: 0.0,
            t_at_peak_vm_s: 0.0,
            step_at_peak_vm: steps,
            min_temp_overall_c: min_temp_overall,
            max_temp_overall_c: max_temp_overall,
            final_min_temp_c: last_sum.min_temp_c,
            final_max_temp_c: last_sum.max_temp_c,
            final_vm_elem: 0.0,
            steady_limit_vm_elem: steady_temp.as_ref().map(|_| 0.0),
            shock_amplification: None,
            timeline,
            frames_temperature,
            frames_u,
            frames_vm_nodal,
            frames_vm_elem,
        };
        return Ok((case_res, report));
    }

    // Build all thermal-expansion load vectors (`steps + 1` transient frames + optional
    // steady limit) and solve them in a SINGLE multi-RHS batch (`K_el` + `TwoLevel`
    // factored once!).
    let rd = ThermalRefData::new();
    let extra = usize::from(steady_temp.is_some());
    let mut rhs_list = Vec::with_capacity(frames.len() + extra);
    let mut dt_frames = Vec::with_capacity(frames.len());
    for fr in &frames {
        let dt_vec: Vec<f64> = fr.temperature.iter().map(|&ti| ti - t_ref).collect();
        rhs_list.push(thermal_load_multi(
            mesh, &dt_vec, young, poisson, alpha_elem, &rd,
        ));
        dt_frames.push(dt_vec);
    }
    let dt_steady = steady_temp.as_ref().map(|ts| {
        let dts: Vec<f64> = ts.iter().map(|&ti| ti - t_ref).collect();
        rhs_list.push(thermal_load_multi(
            mesh, &dts, young, poisson, alpha_elem, &rd,
        ));
        dts
    });

    let iso_mat = Material::iso(young.to_vec(), poisson);
    let elastic_solves =
        solve_multi_rhs(mesh, &iso_mat, fixed, &rhs_list, SolverOptions::default())
            .map_err(SetupError)?;

    let mut peak_vm_elem = -1.0f64;
    let mut peak_von_mises = 0.0f64;
    let mut peak_displacement = 0.0f64;
    let mut step_at_peak_vm = 0usize;
    let mut peak_stress_nodal = Vec::new();
    let mut peak_stress_elem = Vec::new();

    // First pass: evaluate `vm_elem` and `max_d` per frame (cheap O(ne + nn)) and pick peak step,
    // plus recover nodal `von_mises` at every frame for the 3D viewport scrubber.
    for (idx, fr) in frames.iter().enumerate() {
        let (u, erep) = &elastic_solves[idx];
        let dt_vec = &dt_frames[idx];
        let stress_nodal = nodal_stress_thermal_multi(mesh, u, young, poisson, alpha_elem, dt_vec);
        let stress_elem = elem_stress_thermal_multi(mesh, u, young, poisson, alpha_elem, dt_vec);
        let vm_nodal: Vec<f64> = stress_nodal.iter().map(von_mises).collect();
        let vm_elem: Vec<f64> = stress_elem.iter().map(von_mises).collect();
        let max_vm_e = vm_elem.iter().copied().fold(0.0, f64::max);
        let max_vm_n = vm_nodal.iter().copied().fold(0.0, f64::max);
        let max_d = (0..nn)
            .map(|i| (u[3 * i].powi(2) + u[3 * i + 1].powi(2) + u[3 * i + 2].powi(2)).sqrt())
            .fold(0.0, f64::max);

        let min_t = fr.temperature.iter().copied().fold(f64::INFINITY, f64::min);
        let max_t = fr
            .temperature
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let mean_t = fr.temperature.iter().sum::<f64>() / (nn.max(1) as f64);
        min_temp_overall = min_temp_overall.min(min_t);
        max_temp_overall = max_temp_overall.max(max_t);

        if max_vm_e > peak_vm_elem {
            peak_vm_elem = max_vm_e;
            step_at_peak_vm = fr.step;
            peak_stress_nodal = stress_nodal;
            peak_stress_elem = stress_elem;
        }
        peak_von_mises = peak_von_mises.max(max_vm_n);
        peak_displacement = peak_displacement.max(max_d);

        timeline.push(TransientStepSummary {
            step: fr.step,
            time_s: fr.time_s,
            min_temp_c: min_t,
            max_temp_c: max_t,
            mean_temp_c: mean_t,
            max_vm_elem: max_vm_e,
            max_von_mises: max_vm_n,
            max_displacement: max_d,
            conduction_iterations: fr.report.iterations,
            elastic_iterations: erep.iterations,
        });
        frames_temperature.push(fr.temperature.clone());
        frames_u.push(u.clone());
        frames_vm_nodal.push(vm_nodal);
        frames_vm_elem.push(vm_elem);
    }

    let steady_limit_vm_elem = dt_steady.as_ref().map(|dts| {
        let (u_inf, _) = &elastic_solves[frames.len()];
        let se_inf = elem_stress_thermal_multi(mesh, u_inf, young, poisson, alpha_elem, dts);
        se_inf.iter().map(von_mises).fold(0.0, f64::max)
    });
    let shock_amplification = steady_limit_vm_elem.and_then(|s_inf| {
        if s_inf > 1e-6 {
            Some(peak_vm_elem / s_inf)
        } else {
            None
        }
    });

    let p_idx = step_at_peak_vm;
    let peak_fr = &frames[p_idx];
    let peak_sum = &timeline[p_idx];
    let (peak_u, peak_erep) = &elastic_solves[p_idx];
    let peak_force = &rhs_list[p_idx];
    let sigma_min_elem: Vec<f64> = peak_stress_elem.iter().map(min_compressive).collect();
    let grad_t = compute_nodal_temperature_gradient(mesh, &peak_fr.temperature);

    // Sum elastic & conduction iterations across the transient trajectory into `CaseResult.solve`
    // so `Provenance.iterations` reflects the actual linear solver work performed.
    let total_elastic_iters: usize = timeline.iter().map(|s| s.elastic_iterations).sum();
    let mut solve_rep = peak_erep.clone();
    solve_rep.iterations = total_elastic_iters;

    let case_res = CaseResult {
        name: name.clone(),
        max_von_mises: peak_sum.max_von_mises,
        max_vm_elem: peak_sum.max_vm_elem,
        max_displacement: peak_sum.max_displacement,
        compliance: compliance(peak_force, peak_u),
        compliance_upper: None,
        u: peak_u.clone(),
        stress: peak_stress_nodal,
        von_mises: frames_vm_nodal[p_idx].clone(),
        sigma_min_elem,
        stress_elem: peak_stress_elem,
        plastic_strain_elem: vec![0.0; ne],
        grad_t_nodal: Some(grad_t),
        peel_nodal: Vec::new(),
        peel_elem: vec![0.0; ne],
        max_layer_peel: 0.0,
        contact_pressure: Vec::new(),
        vm_elem: frames_vm_elem[p_idx].clone(),
        solve: solve_rep,
        load_reports: Vec::new(),
        contact_reports: Vec::new(),
        temperature: Some(peak_fr.temperature.clone()),
        conduction: Some(peak_fr.report.clone()),
    };

    let last_sum = timeline.last().expect("at least 2 summaries");
    let report = TransientCaseReport {
        case_name: name,
        scheme: if steps >= 2 { "bdf2" } else { "bdf1" }.into(),
        t_init_c: t_init,
        t_ref_c: t_ref,
        t_end_s: t_end,
        dt_s: t_end / (steps as f64),
        steps_count: steps,
        diffusivity_mm2_s,
        peak_vm_elem,
        peak_von_mises,
        peak_displacement,
        t_at_peak_vm_s: peak_fr.time_s,
        step_at_peak_vm,
        min_temp_overall_c: min_temp_overall,
        max_temp_overall_c: max_temp_overall,
        final_min_temp_c: last_sum.min_temp_c,
        final_max_temp_c: last_sum.max_temp_c,
        final_vm_elem: last_sum.max_vm_elem,
        steady_limit_vm_elem,
        shock_amplification,
        timeline,
        frames_temperature,
        frames_u,
        frames_vm_nodal,
        frames_vm_elem,
    };

    Ok((case_res, report))
}

/// Standalone transient thermo-elastic solver for API `QuestionSpec::Transient`, MCP
/// `transient_solve`, and WASM `solveTransientFsm`, accepting optional overrides on `t_end`,
/// `steps`, and `t_init` over `doc.solve`.
pub fn analyse_transient(
    doc: &Document,
    res: Resolution,
    clock: Clock,
    t_end_override: Option<f64>,
    steps_override: Option<usize>,
    t_init_override: Option<f64>,
) -> Result<(Provenance, Vec<TransientCaseReport>), SetupError> {
    let sys_resolved = crate::declarative_system::resolve_system_doc(doc)?;
    let sdoc = structural_doc(sys_resolved.as_ref());
    let doc = sdoc.as_ref();
    let thermal_cases = doc.thermal_cases();
    if thermal_cases.is_empty() {
        return Err(SetupError(
            "transient solve requires at least one thermal boundary condition (temperature, convection, or heatflux)".into(),
        ));
    }
    let t_end = t_end_override.or(doc.solve.t_end).unwrap_or(10.0);
    let steps = steps_override
        .or(doc.solve.steps)
        .unwrap_or(DEFAULT_TRANSIENT_STEPS);
    let t_init = t_init_override
        .or(doc.solve.t_init)
        .or(doc.solve.t_ref)
        .unwrap_or(20.0);

    let Prepared {
        mesh,
        facets,
        fixed,
        mat,
        young,
        k_phys_elem,
        c_vol_elem,
        alpha_elem,
        h,
        quality,
        mesh_ms,
        ..
    } = prepare(doc, res, clock, false)?;

    let has_supports = fixed.iter().any(|&m| m != 0);
    let mut alphas = Vec::with_capacity(alpha_elem.len());
    if has_supports {
        let _ = mat.alpha.ok_or_else(|| {
            SetupError(format!(
                "The thermal flow needs a thermal-expansion coefficient and material '{}' \
                 states none — pick a material whose sheet states a CTE, or give a custom \
                 material an a= value",
                doc.material
            ))
        })?;
        for &a in &alpha_elem {
            let av = a.ok_or_else(|| {
                SetupError(
                    "The thermal flow needs a thermal-expansion coefficient on every assigned \
                     volume material — pick a material whose sheet states a CTE, or give a custom \
                     material an a= value"
                        .into(),
                )
            })?;
            alphas.push(av);
        }
    } else {
        alphas.resize(alpha_elem.len(), 0.0);
    }

    let diffusivity_mm2_s =
        mat.effective_k_w_mm_k() / mat.volumetric_heat_capacity_j_mm3_k().max(1e-18);
    let t_start = clock();
    let mut reports = Vec::with_capacity(thermal_cases.len());
    let mut iterations = 0usize;
    for name in thermal_cases {
        let (case_res, rep) = run_transient_thermal_case_multi_with_regions(
            doc,
            &mesh,
            &young,
            mat.nu,
            &alphas,
            &fixed,
            &facets,
            name,
            h,
            &k_phys_elem,
            &c_vol_elem,
            diffusivity_mm2_s,
            t_init,
            t_end,
            steps,
            None,
        )?;
        iterations += case_res.solve.iterations
            + rep
                .timeline
                .iter()
                .map(|s| s.conduction_iterations)
                .sum::<usize>();
        reports.push(rep);
    }
    let thermal_ms = clock() - t_start;
    let prov = Provenance {
        elements: mesh.tets.len(),
        order: 2,
        nodes: mesh.nodes.len(),
        dofs: mesh.nodes.len() * 3,
        element_size: h,
        mesh_ms,
        solve_ms: thermal_ms,
        thermal_ms,
        total_ms: mesh_ms + thermal_ms,
        quality,
        iterations,
    };
    Ok((prov, reports))
}
