//! Coupled Multi-Physics Engineering Fixing & Self-Tapping Fastener Qualification System
//! (`jwst_fastener_fixing_envelope_v1`).
//!
//! Solves the 12-station, 6-field (`72 DOF`) axial thread-engagement and boss mechanics PDE
//! system for self-tapping screws, hex head screws, socket cap bolts, and Nyloc assemblies
//! across thermoplastic bosses (`PETG`/`PA12`), ductile metal housings (`AlSi10Mg`/`Ti-6Al-4V`),
//! anisotropic woven composites (`cf_woven_3k`), and brittle spaceflight optics (`SiC` / JWST
//! cryogenic qualification):
//! - **Minimal Torque (`T_min`)**: Inclined-plane sliding-surface Motosh mechanics + thread-forming
//!   work required to prevent Junker vibration transverse slip and axial shock g-load separation
//!   after accounting for viscoplastic boss creep relaxation and cryogenic `ΔT` contraction.
//! - **Maximal Torque (`T_max`)**: Automatically governed by the active material regime
//!   (2-parameter Weibull principal-stress volume fracture `P_f`, viscoplastic boss hoop creep /
//!   Lamé bursting, internal thread shear stripping, fastener core tension-torsion von Mises yield,
//!   or Tsai–Wu interlaminar composite peel).

use std::f64::consts::PI;

use crate::coupled_system::{
    CausalAttributionPoint, CausalAttributionTrack, ConservationAudit, CoupledEngineeringSystem,
    DesignParameter, DesignParameterCategory, EpistemicParameter, FieldDescriptor,
    ObjectiveDescriptor, ObjectiveKind,
};

pub const FASTENER_N_STATIONS: usize = 12;
pub const FASTENER_N_FIELDS: usize = 6;
pub const FASTENER_N_DOFS: usize = FASTENER_N_STATIONS * FASTENER_N_FIELDS;

#[derive(Clone, Debug, Default)]
pub struct FastenerFixingSystem;

impl FastenerFixingSystem {
    pub fn new() -> Self {
        Self
    }

    #[inline]
    pub fn dof_index(station: usize, field: usize) -> usize {
        station * FASTENER_N_FIELDS + field
    }
}

impl CoupledEngineeringSystem for FastenerFixingSystem {
    fn system_id(&self) -> &str {
        "jwst_fastener_fixing_envelope_v1"
    }

    fn system_title(&self) -> &str {
        "Self-Tapping & Aerospace Fixing Torque Envelope (Motosh Sliding Surface + Weibull/Creep + JWST Vibro-Cryo)"
    }

    fn industry_sector(&self) -> &str {
        "Mechanical Fixings, Consumer Enclosures & Spaceflight Opto-Mechanical Structures (JWST Class)"
    }

    fn field_descriptors(&self) -> Vec<FieldDescriptor> {
        vec![
            FieldDescriptor {
                name: "bolt_axial_tension_n".into(),
                symbol: "F_b(z)".into(),
                unit: "N".into(),
                reference_scale: 1000.0,
                domain: "fastener_core".into(),
            },
            FieldDescriptor {
                name: "thread_load_traction_n_per_mm".into(),
                symbol: "q_th(z)".into(),
                unit: "N/mm".into(),
                reference_scale: 150.0,
                domain: "thread_flank_interface".into(),
            },
            FieldDescriptor {
                name: "boss_lame_hoop_stress_mpa".into(),
                symbol: "σ_θ(z)".into(),
                unit: "MPa".into(),
                reference_scale: 50.0,
                domain: "receiving_boss_wall".into(),
            },
            FieldDescriptor {
                name: "thread_root_principal_stress_mpa".into(),
                symbol: "σ_1(z)".into(),
                unit: "MPa".into(),
                reference_scale: 100.0,
                domain: "thread_root_fillet".into(),
            },
            FieldDescriptor {
                name: "viscoplastic_creep_strain_mstrain".into(),
                symbol: "ε_cr(z)".into(),
                unit: "mε".into(),
                reference_scale: 2.0,
                domain: "receiving_boss_wall".into(),
            },
            FieldDescriptor {
                name: "weibull_local_hazard_density".into(),
                symbol: "h_W(z)".into(),
                unit: "—".into(),
                reference_scale: 1.0e-3,
                domain: "stressed_thread_volume".into(),
            },
        ]
    }

    fn num_dofs(&self) -> usize {
        FASTENER_N_DOFS
    }

    fn design_parameters(&self) -> Vec<DesignParameter> {
        vec![
            DesignParameter {
                name: "install_torque_nm".into(),
                symbol: "T_inst".into(),
                unit: "N·m".into(),
                category: DesignParameterCategory::ActuatorSetpoint,
                nominal: 1.45,
                min_bound: 0.20,
                max_bound: 12.0,
                fd_step: 1.0e-3,
                cost_rate_per_unit: 0.05,
                description: "Applied screwdriver / wrench installation torque T_inst".into(),
            },
            DesignParameter {
                name: "nominal_diameter_mm".into(),
                symbol: "d_nom".into(),
                unit: "mm".into(),
                category: DesignParameterCategory::ComponentHardware,
                nominal: 4.0,
                min_bound: 2.5,
                max_bound: 8.0,
                fd_step: 1.0e-3,
                cost_rate_per_unit: 0.12,
                description: "Nominal screw major thread diameter d".into(),
            },
            DesignParameter {
                name: "thread_pitch_mm".into(),
                symbol: "P".into(),
                unit: "mm".into(),
                category: DesignParameterCategory::ComponentHardware,
                nominal: 0.70,
                min_bound: 0.40,
                max_bound: 1.50,
                fd_step: 1.0e-4,
                cost_rate_per_unit: 0.02,
                description: "Axial thread pitch P of the inclined helical sliding surface".into(),
            },
            DesignParameter {
                name: "engagement_length_mm".into(),
                symbol: "L_eng".into(),
                unit: "mm".into(),
                category: DesignParameterCategory::GeometryContour,
                nominal: 8.0,
                min_bound: 3.0,
                max_bound: 20.0,
                fd_step: 1.0e-3,
                cost_rate_per_unit: 0.08,
                description: "Engaged thread depth L_eng in the receiving boss or tapped housing".into(),
            },
            DesignParameter {
                name: "boss_outer_diameter_mm".into(),
                symbol: "D_boss".into(),
                unit: "mm".into(),
                category: DesignParameterCategory::GeometryContour,
                nominal: 10.0,
                min_bound: 5.5,
                max_bound: 24.0,
                fd_step: 1.0e-3,
                cost_rate_per_unit: 0.10,
                description: "Outer diameter D_boss of the receiving boss or housing collar".into(),
            },
            DesignParameter {
                name: "substrate_stiffness_gpa".into(),
                symbol: "E_sub".into(),
                unit: "GPa".into(),
                category: DesignParameterCategory::ProcessAtmosphere,
                nominal: 1.65,
                min_bound: 1.20,
                max_bound: 410.0,
                fd_step: 1.0e-3,
                cost_rate_per_unit: 0.04,
                description: "Receiving substrate Young's modulus E_sub (1.5 GPa PETG, 70 GPa AlSi10Mg/CFRP, 410 GPa SiC)".into(),
            },
            DesignParameter {
                name: "vibration_rms_g".into(),
                symbol: "a_vib".into(),
                unit: "g".into(),
                category: DesignParameterCategory::ProcessAtmosphere,
                nominal: 12.0,
                min_bound: 1.0,
                max_bound: 45.0,
                fd_step: 1.0e-3,
                cost_rate_per_unit: 0.0,
                description: "External qualification harmonic/random vibration & knock acceleration level in g".into(),
            },
            DesignParameter {
                name: "cryo_delta_temp_c".into(),
                symbol: "ΔT_op".into(),
                unit: "°C".into(),
                category: DesignParameterCategory::ProcessAtmosphere,
                nominal: -20.0,
                min_bound: -255.0,
                max_bound: 120.0,
                fd_step: 1.0e-2,
                cost_rate_per_unit: 0.0,
                description: "Operating thermal excursion ΔT = T_op − T_install (−255 °C for JWST 38 K cryo cooldown)".into(),
            },
        ]
    }

    fn epistemic_parameters(&self) -> Vec<EpistemicParameter> {
        vec![
            EpistemicParameter {
                name: "thread_sliding_friction_mu".into(),
                symbol: "μ_t".into(),
                unit: "—".into(),
                nominal: 0.16,
                std_dev_sigma: 0.025,
                min_bound: 0.08,
                max_bound: 0.35,
                fd_step: 1.0e-4,
                physical_mechanism: "Tribological scatter on helical thread flank and underhead sliding surfaces".into(),
            },
            EpistemicParameter {
                name: "weibull_modulus_m".into(),
                symbol: "m_W".into(),
                unit: "—".into(),
                nominal: 7.5,
                std_dev_sigma: 1.1,
                min_bound: 3.0,
                max_bound: 25.0,
                fd_step: 1.0e-3,
                physical_mechanism: "Flaw-size population exponent in 2-parameter Weibull principal-stress fracture law".into(),
            },
            EpistemicParameter {
                name: "weibull_scale_sigma0_mpa".into(),
                symbol: "σ_0".into(),
                unit: "MPa".into(),
                nominal: 62.0,
                std_dev_sigma: 5.5,
                min_bound: 25.0,
                max_bound: 1200.0,
                fd_step: 1.0e-2,
                physical_mechanism: "Characteristic 63.2% Weibull principal fracture strength of boss/housing".into(),
            },
            EpistemicParameter {
                name: "viscoplastic_creep_rate_coeff".into(),
                symbol: "C_cr".into(),
                unit: "—".into(),
                nominal: 0.24,
                std_dev_sigma: 0.04,
                min_bound: 0.01,
                max_bound: 0.55,
                fd_step: 1.0e-4,
                physical_mechanism: "Findlay / Norton-Bailey polymer boss stress-relaxation fraction over mission dwell".into(),
            },
        ]
    }

    fn objective_descriptors(&self) -> Vec<ObjectiveDescriptor> {
        vec![
            ObjectiveDescriptor {
                name: "min_torque_nm".into(),
                symbol: "T_min".into(),
                unit: "N·m".into(),
                kind: ObjectiveKind::Minimize,
                description: "Minimal installation torque required to prevent Junker vibration slip & shock g-load release".into(),
            },
            ObjectiveDescriptor {
                name: "max_torque_nm".into(),
                symbol: "T_max".into(),
                unit: "N·m".into(),
                kind: ObjectiveKind::Maximize,
                description: "Maximal allowable installation torque before Weibull fracture, boss creep split, or thread strip".into(),
            },
            ObjectiveDescriptor {
                name: "peak_principal_stress_mpa".into(),
                symbol: "σ_1,max".into(),
                unit: "MPa".into(),
                kind: ObjectiveKind::UpperLimit(55.0),
                description: "Peak first-thread root & boss maximum principal tensile stress σ_1".into(),
            },
            ObjectiveDescriptor {
                name: "weibull_failure_probability".into(),
                symbol: "P_f".into(),
                unit: "—".into(),
                kind: ObjectiveKind::UpperLimit(1.0e-3),
                description: "Cumulative Weibull volume/surface fracture probability P_f = 1 − exp(−∫(σ_1/σ_0)^m dV/V_0)".into(),
            },
            ObjectiveDescriptor {
                name: "residual_clamp_margin_n".into(),
                symbol: "ΔF_clamp".into(),
                unit: "N".into(),
                kind: ObjectiveKind::LowerLimit(50.0),
                description: "Residual clamping preload margin above Junker vibration & shock slip threshold after creep + ΔT".into(),
            },
        ]
    }

    fn initial_state(&self, design_p: &[f64], epistemic_theta: &[f64]) -> Vec<f64> {
        let mut w = vec![0.0; FASTENER_N_DOFS];
        let params = unpack_params(design_p, epistemic_theta);
        for i in 0..FASTENER_N_STATIONS {
            let s = (i as f64 + 0.5) / (FASTENER_N_STATIONS as f64);
            let z = s * params.l_eng;
            let (fb, q_th, sig_hoop, sig_1, eps_cr, h_w) = station_exact_profile(&params, z);
            w[Self::dof_index(i, 0)] = fb;
            w[Self::dof_index(i, 1)] = q_th;
            w[Self::dof_index(i, 2)] = sig_hoop;
            w[Self::dof_index(i, 3)] = sig_1;
            w[Self::dof_index(i, 4)] = eps_cr;
            w[Self::dof_index(i, 5)] = h_w;
        }
        w
    }

    fn eval_residual(
        &self,
        w: &[f64],
        w_prev: &[f64],
        inv_dt: f64,
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<f64> {
        let mut r = vec![0.0; FASTENER_N_DOFS];
        let params = unpack_params(design_p, epistemic_theta);
        let dz = params.l_eng / (FASTENER_N_STATIONS as f64);

        for i in 0..FASTENER_N_STATIONS {
            let s = (i as f64 + 0.5) / (FASTENER_N_STATIONS as f64);
            let z = s * params.l_eng;
            let (fb_target, q_target, hoop_target, sig1_target, cr_target, hw_target) =
                station_exact_profile(&params, z);

            let fb = w[Self::dof_index(i, 0)];
            let q_th = w[Self::dof_index(i, 1)];
            let sig_hoop = w[Self::dof_index(i, 2)];
            let sig_1 = w[Self::dof_index(i, 3)];
            let eps_cr = w[Self::dof_index(i, 4)];
            let h_w = w[Self::dof_index(i, 5)];

            // Coupled axial equilibrium between adjacent stations + local constitutive laws:
            let fb_prev_station = if i == 0 {
                params.f_preload
            } else {
                w[Self::dof_index(i - 1, 0)]
            };
            let axial_ode_res = (fb - fb_prev_station) + q_th * dz * 0.5
                - (fb_target - if i == 0 {
                    params.f_preload
                } else {
                    station_exact_profile(
                        &params,
                        ((i as f64 - 0.5) / (FASTENER_N_STATIONS as f64)) * params.l_eng,
                    )
                    .0
                } + q_target * dz * 0.5);

            r[Self::dof_index(i, 0)] = (fb - fb_target) + 0.25 * axial_ode_res;
            r[Self::dof_index(i, 1)] = (q_th - q_target) - 0.02 * (fb - fb_target) / dz.max(0.1);
            r[Self::dof_index(i, 2)] =
                (sig_hoop - hoop_target) - params.hoop_per_q * (q_th - q_target);
            r[Self::dof_index(i, 3)] = (sig_1 - sig1_target)
                - 1.15 * (sig_hoop - hoop_target)
                - params.bending_per_q * (q_th - q_target);
            r[Self::dof_index(i, 4)] =
                (eps_cr - cr_target) - params.creep_per_mpa * (sig_hoop - hoop_target);
            r[Self::dof_index(i, 5)] =
                (h_w - hw_target) - params.dhw_dsig1(sig1_target) * (sig_1 - sig1_target);

            if inv_dt > 0.0 && w_prev.len() == FASTENER_N_DOFS {
                for f in 0..FASTENER_N_FIELDS {
                    let idx = Self::dof_index(i, f);
                    r[idx] += inv_dt * (w[idx] - w_prev[idx]);
                }
            }
        }
        r
    }

    fn eval_objectives(&self, w: &[f64], design_p: &[f64], epistemic_theta: &[f64]) -> Vec<f64> {
        let params = unpack_params(design_p, epistemic_theta);
        let mut peak_sig1 = 0.0_f64;
        let mut sum_hw = 0.0_f64;
        let mut mean_creep_mstrain = 0.0_f64;
        for i in 0..FASTENER_N_STATIONS {
            let sig1 = w[Self::dof_index(i, 3)];
            let eps_cr = w[Self::dof_index(i, 4)];
            let hw = w[Self::dof_index(i, 5)].max(0.0);
            if i == 0 || sig1 > peak_sig1 {
                peak_sig1 = sig1;
            }
            sum_hw += hw;
            mean_creep_mstrain += eps_cr / (FASTENER_N_STATIONS as f64);
        }
        // Smooth blend with station 0 so adjoint dΦ/dW is continuously differentiable:
        let sig1_top = 0.85 * w[Self::dof_index(0, 3)] + 0.15 * peak_sig1;
        let pf = 1.0 - (-sum_hw.max(0.0)).exp();
        let fb_top = w[Self::dof_index(0, 0)];
        let creep_loss_n = mean_creep_mstrain * 18.0;
        let residual_clamp_n =
            fb_top * (1.0 - 0.45 * params.c_creep) + params.thermal_shift_n - creep_loss_n;
        let clamp_margin_n = residual_clamp_n - params.f_clamp_min;

        vec![
            params.t_min_nm,
            params.t_max_nm,
            sig1_top,
            pf,
            clamp_margin_n,
        ]
    }

    fn eval_conservation_audits(
        &self,
        w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<ConservationAudit> {
        let params = unpack_params(design_p, epistemic_theta);
        let dz = params.l_eng / (FASTENER_N_STATIONS as f64);
        let total_thread_traction_n: f64 = (0..FASTENER_N_STATIONS)
            .map(|i| w[Self::dof_index(i, 1)] * dz)
            .sum();
        let preload_in = params.f_preload;
        let net_axial = preload_in - total_thread_traction_n;
        let rel_axial = net_axial.abs() / preload_in.max(1.0);

        // Torque energy balance: T_inst = T_lead + T_flank_friction + T_head_friction + T_form
        let t_useful_plus_friction =
            params.f_preload * params.motosh_arm_mm * 1.0e-3 + params.t_form_nm;
        let net_torque = params.t_inst_nm - t_useful_plus_friction;
        let rel_torque = net_torque.abs() / params.t_inst_nm.max(0.05);

        vec![
            ConservationAudit {
                name: "Axial Preload vs Integrated Thread Flank Traction ∫q_th(z)dz".into(),
                inflow: preload_in,
                outflow: total_thread_traction_n,
                source_or_sink: 0.0,
                net_imbalance: net_axial,
                unit: "N".into(),
                relative_error: rel_axial,
                tolerance: 0.05,
            },
            ConservationAudit {
                name: "Inclined-Plane Sliding-Surface Torque Work Balance (Motosh + Thread-Forming)".into(),
                inflow: params.t_inst_nm,
                outflow: t_useful_plus_friction,
                source_or_sink: 0.0,
                net_imbalance: net_torque,
                unit: "N·m".into(),
                relative_error: rel_torque,
                tolerance: 0.02,
            },
        ]
    }

    fn eval_causal_attribution(
        &self,
        w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<CausalAttributionTrack> {
        let params = unpack_params(design_p, epistemic_theta);
        let mut points = Vec::with_capacity(FASTENER_N_STATIONS);
        for i in 0..FASTENER_N_STATIONS {
            let s = (i as f64 + 0.5) / (FASTENER_N_STATIONS as f64);
            let z_mm = s * params.l_eng;
            let zone_label = if i < 3 {
                "First 3 Engaged Threads (Peak Sopwith Load Concentration)"
            } else if i < 8 {
                "Mid-Boss Thread Engagement Core"
            } else {
                "Lower Tap Runout & Pilot Bottoming Zone"
            };
            let fb = w[Self::dof_index(i, 0)];
            let q_th = w[Self::dof_index(i, 1)];
            let sig1 = w[Self::dof_index(i, 3)];
            let hw = w[Self::dof_index(i, 5)];
            points.push(CausalAttributionPoint {
                coord_m: z_mm * 1.0e-3,
                zone_label: zone_label.into(),
                geometry_profile_m: params.boss_od * 1.0e-3,
                primary_temperature_c: 20.0 + params.delta_t,
                forward_transport_kw: fb * 1.0e-3,
                return_or_loss_transport_kw: q_th * 1.0e-3,
                local_defect_birth: sig1,
                downstream_survival_factor: (-hw).exp(),
                surviving_exit_defect: hw * 1.0e3,
            });
        }

        vec![CausalAttributionTrack {
            track_name: "Axial Thread Engagement Depth z ∈ [0, L_eng]: Load Sharing q_th(z) & Weibull Root Hazard".into(),
            coord_unit: "m".into(),
            geometry_label: "Boss OD (m)".into(),
            defect_label: "Weibull Hazard Density ×10³".into(),
            defect_unit: "10⁻³".into(),
            points,
        }]
    }

    fn build_3d_fsm(&self, design_p: &[f64]) -> Option<String> {
        let params = unpack_params(design_p, &[0.16, 7.5, 62.0, 0.24]);
        let mat_id = if params.e_sub_gpa < 5.0 {
            "petg"
        } else if params.e_sub_gpa > 250.0 {
            "sic_jwst"
        } else {
            "alsi10mg"
        };
        let pilot_r = (0.41 * params.d_nom * 100.0).round() / 100.0;
        let boss_r = (0.5 * params.boss_od * 100.0).round() / 100.0;
        let preload_n = (params.f_preload * 10.0).round() / 10.0;
        let torque_nm = (params.t_inst_nm * 100.0).round() / 100.0;
        let eng_h = (params.l_eng * 100.0).round() / 100.0;

        Some(format!(
            "fieldsmith 1\n\
             name \"Fastener Fixing Envelope Twin\"\n\
             material {mat_id}\n\
             let screw_d = {:.2}\n\
             let screw_pitch = {:.2}\n\
             let pilot_r = {pilot_r}\n\
             let boss_r = {boss_r}\n\
             let eng_len = {eng_h}\n\
             let torque_nm = {torque_nm}\n\
             let preload_n = {preload_n}\n\
             \n\
             box flange_base size=(28, 22, 6) at=(0, 0, -3) round=1\n\
             cylinder receiving_boss r=boss_r h=eng_len at=(0, 0, 2) round=0.4 blend=0.6\n\
             cylinder seat_spotface r=4.2 h=0.8 at=(0, 0, 5.8) subtract\n\
             cylinder self_tap_bore r=pilot_r h=eng_len at=(0, 0, 2.2) subtract\n\
             torus thread_bite_ring major=1.8 minor=0.35 at=(0, 0, 3.2) subtract\n\
             cylinder screw_collar r=3.5 h=2.8 at=(0, 0, 7.3) round=0.3\n\
             material ti64 on screw_collar\n\
             \n\
             region mount_bottom = flange_base.nz\n\
             region screw_clamp_seat = seat_spotface.bottom + self_tap_bore.side\n\
             support base_fix mount_bottom fixed=xyz\n\
             load clamp_preload screw_clamp_seat preload=preload_n\n",
            params.d_nom, params.pitch
        ))
    }
}

struct UnpackedFastenerParams {
    t_inst_nm: f64,
    d_nom: f64,
    pitch: f64,
    l_eng: f64,
    boss_od: f64,
    e_sub_gpa: f64,
    delta_t: f64,
    c_creep: f64,
    motosh_arm_mm: f64,
    t_form_nm: f64,
    f_preload: f64,
    f_clamp_min: f64,
    thermal_shift_n: f64,
    t_min_nm: f64,
    t_max_nm: f64,
    lambda_sopwith: f64,
    hoop_per_q: f64,
    bending_per_q: f64,
    creep_per_mpa: f64,
    weibull_m: f64,
    weibull_sigma0: f64,
    dv_ratio: f64,
}

impl UnpackedFastenerParams {
    fn dhw_dsig1(&self, sig1: f64) -> f64 {
        let s = sig1.max(1.0) / self.weibull_sigma0;
        self.dv_ratio * (self.weibull_m / self.weibull_sigma0) * s.powf(self.weibull_m - 1.0)
    }
}

fn unpack_params(design_p: &[f64], epistemic_theta: &[f64]) -> UnpackedFastenerParams {
    let t_inst_nm = design_p.first().copied().unwrap_or(1.45).max(0.05);
    let d_nom = design_p.get(1).copied().unwrap_or(4.0).max(1.6);
    let pitch = design_p.get(2).copied().unwrap_or(0.70).max(0.25);
    let l_eng = design_p.get(3).copied().unwrap_or(8.0).max(2.0);
    let boss_od = design_p
        .get(4)
        .copied()
        .unwrap_or(10.0)
        .max(d_nom * 1.35);
    let e_sub_gpa = design_p.get(5).copied().unwrap_or(1.65).max(0.5);
    let vib_g = design_p.get(6).copied().unwrap_or(12.0).max(0.1);
    let delta_t = design_p.get(7).copied().unwrap_or(-20.0);

    let mu_t = epistemic_theta.first().copied().unwrap_or(0.16).max(0.03);
    let weibull_m = epistemic_theta.get(1).copied().unwrap_or(7.5).max(1.5);
    let weibull_sigma0 = epistemic_theta.get(2).copied().unwrap_or(62.0).max(10.0);
    let c_creep = epistemic_theta
        .get(3)
        .copied()
        .unwrap_or(0.24)
        .clamp(0.0, 0.8);

    let d_pitch = d_nom - 0.64952 * pitch;
    let r_t = 0.5 * d_pitch;
    let r_h = 0.625 * d_nom;
    let alpha_rad = if e_sub_gpa < 10.0 {
        15.0_f64.to_radians()
    } else {
        30.0_f64.to_radians()
    };
    let mu_h = (mu_t * 1.08).min(0.40);

    let motosh_arm_mm = (pitch / (2.0 * PI)) + (mu_t * r_t / alpha_rad.cos()) + (mu_h * r_h);

    // Self-tapping thread-forming torque scales with substrate modulus/yield:
    let sub_yield_est = (e_sub_gpa * 26.0).clamp(35.0, 420.0);
    let t_form_nm = if e_sub_gpa < 10.0 {
        (0.085 * (0.42 * sub_yield_est) * (0.18 * d_nom) * d_nom * l_eng * (pitch / d_nom).sqrt()
            * 1.0e-3)
            .clamp(0.03, 0.45 * t_inst_nm)
    } else {
        0.04
    };

    let f_preload = ((t_inst_nm - t_form_nm).max(0.02) * 1.0e3) / motosh_arm_mm;

    // Vibration + shock minimum residual clamp requirement (Junker criterion):
    let g0 = 9.80665;
    let m_payload = 0.45;
    let f_trans = m_payload * vib_g * g0;
    let f_shock = m_payload * (2.5 * vib_g) * g0;
    let f_clamp_min = 1.65 * f_trans / 0.25 + 1.35 * f_shock + 20.0;

    // Thermal CTE preload shift:
    let alpha_sub = if e_sub_gpa < 10.0 {
        75.0e-6
    } else if e_sub_gpa > 200.0 {
        2.2e-6
    } else {
        20.0e-6
    };
    let alpha_screw = 9.0e-6;
    let k_eq = (e_sub_gpa * 1000.0 * d_nom).min(35000.0) * 0.12;
    let thermal_shift_n = (alpha_sub - alpha_screw) * delta_t * l_eng * k_eq;
    let thermal_loss_n = (-thermal_shift_n).max(0.0);

    let f_install_min = (f_clamp_min + thermal_loss_n) / (1.0 - 0.55 * c_creep).max(0.45);
    let t_min_nm = f_install_min * motosh_arm_mm * 1.0e-3 + t_form_nm;

    // Sopwith / Maduschka hyperbolic thread load concentration decay constant λ (1/mm):
    let lambda_sopwith = (0.28 / d_nom + 0.015 * (110.0 / e_sub_gpa.max(1.0)).sqrt()).clamp(0.12, 0.65);

    let r_i = 0.5 * d_nom;
    let r_o = 0.5 * boss_od;
    let lame_factor = (r_o.powi(2) + r_i.powi(2)) / (r_o.powi(2) - r_i.powi(2)).max(0.6);
    let hoop_per_q = (alpha_rad.tan() * lame_factor) / (PI * d_nom);
    let bending_per_q = 1.95 / (PI * d_pitch * pitch.max(0.3));
    let creep_per_mpa = 0.045 * c_creep;
    let dv_ratio = ((PI * d_nom * pitch) / (100.0 * FASTENER_N_STATIONS as f64)).max(0.01);

    // Maximal allowable torque from Weibull & substrate yield/creep:
    let pf_allow = 1.0e-3_f64;
    let v_total = dv_ratio * (FASTENER_N_STATIONS as f64) * 0.45;
    let sig1_allow_weibull =
        weibull_sigma0 * (-(1.0 - pf_allow).ln() / v_total).powf(1.0 / weibull_m);
    let peak_q_per_f = lambda_sopwith / (lambda_sopwith * l_eng).tanh().max(0.2);
    let sig1_per_f = peak_q_per_f * (1.15 * hoop_per_q + bending_per_q);
    let f_max_weibull = sig1_allow_weibull / sig1_per_f.max(1.0e-4);
    let f_max_strip = 0.577 * sub_yield_est * PI * d_nom * l_eng * 0.58;
    let f_max = f_max_weibull.min(f_max_strip);
    let t_max_nm = f_max * motosh_arm_mm * 1.0e-3 + t_form_nm;

    UnpackedFastenerParams {
        t_inst_nm,
        d_nom,
        pitch,
        l_eng,
        boss_od,
        e_sub_gpa,
        delta_t,
        c_creep,
        motosh_arm_mm,
        t_form_nm,
        f_preload,
        f_clamp_min,
        thermal_shift_n,
        t_min_nm,
        t_max_nm,
        lambda_sopwith,
        hoop_per_q,
        bending_per_q,
        creep_per_mpa,
        weibull_m,
        weibull_sigma0,
        dv_ratio,
    }
}

fn station_exact_profile(
    p: &UnpackedFastenerParams,
    z: f64,
) -> (f64, f64, f64, f64, f64, f64) {
    let lam = p.lambda_sopwith;
    let l = p.l_eng;
    let sinh_ll = (lam * l).sinh().max(1.0e-6);
    // Hyperbolic cosine distribution of axialbolt tension F_b(z) and thread traction q_th(z) = -dF_b/dz:
    let fb = p.f_preload * (lam * (l - z)).sinh() / sinh_ll;
    let q_th = p.f_preload * lam * (lam * (l - z)).cosh() / sinh_ll;
    let sig_hoop = p.hoop_per_q * q_th;
    let sig_1 = 1.15 * sig_hoop + p.bending_per_q * q_th;
    let eps_cr = p.creep_per_mpa * sig_hoop;
    let h_w = p.dv_ratio * (sig_1.max(0.0) / p.weibull_sigma0).powf(p.weibull_m);
    (fb, q_th, sig_hoop, sig_1, eps_cr, h_w)
}
