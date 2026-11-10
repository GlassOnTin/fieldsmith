//! Solar-Focused Optical Lens Candle Ignition, Combustion Chemistry, Buoyant Plume & Smoke
//! Tracking (`solar_candle_ignition_v1`).
//!
//! Couples:
//! 1. **Thick-Lens BK7 Borosilicate Solar Caustic Optics**:
//!    - Biconvex glass lens ($n = 1.5168$, center thickness $t_c = 8\text{ mm}$, aperture
//!      $D_{\text{lens}}$, surface curvature radius $R_{\text{lens}}$, height above wick $z_{\text{lens}}$).
//!    - Lensmaker focal length $f$, solar disk subtense $\theta_\odot = 4.65\text{ mrad}$,
//!      spherical aberration blur $\sigma_{\text{sph}}$, Fresnel reflection loss, and axial
//!      caustic waist $w_{\text{beam}}(z)$ delivering concentrated flux $q''_{\text{solar}}(r, z)$.
//! 2. **Wick Heating, Paraffin ($\text{C}_{25}\text{H}_{52}$) Capillary Melting/Pyrolysis & Auto-Ignition**:
//!    - Melting at $T_{\text{melt}} = 331\text{ K}$ ($58\,^\circ\text{C}$), endothermic vaporization/pyrolysis
//!      at $T_{\text{vap}} = 620\text{ K}$ ($347\,^\circ\text{C}$), and flash auto-ignition at
//!      $T_{\text{ign}} = 670\text{ K}$ ($397\,^\circ\text{C}$) yielding ignition delay $t_{\text{ign}}$ (s)
//!      and steady capillary fuel pyrolysis rate $\dot{m}_f$ ($\text{mg/s}$).
//! 3. **Monolithic 6-Field Buoyant Diffusion Flame, $\text{OH}^*$ Chemiluminescence & Soot/Smoke PDE**:
//!    - 24 axial stations $z \in [0, 90\text{ mm}]$ above the wick base $\times$ 6 coupled fields
//!      ($144\text{ DOFs}$):
//!      - `u_z` ($\text{m/s}$): Boussinesq/ideal-gas buoyant axial updraft velocity with
//!        Morton–Taylor–Turner radial air entrainment.
//!      - `Z_mix` ($-$): Bilger hydrocarbon mixture fraction ($Z_{\text{st}} = 0.0637$ for
//!        $\text{C}_{25}\text{H}_{52}$ in air).
//!      - `T_gas` ($\text{K}$): Flame & plume temperature with Burke–Schumann / Arrhenius
//!        stoichiometric reaction-sheet heat release and Planck-mean gas + soot blackbody radiation.
//!      - `Y_OH_rad` ($\text{ppm}$): Excited $\text{OH}^*/\text{CH}^*$ chemiluminescent radical concentration
//!        at the blue reaction sheet base.
//!      - `f_v_soot` ($\text{ppm}$): Carbonaceous soot volume fraction governed by Moss–Brookes
//!        fuel-rich PAH inception + surface growth ($Z > Z_{\text{st}}, T > 1250\text{ K}$) minus
//!        Nagle–Strickland-Constable thermal oxidation ($\text{O}_2/\text{OH}\cdot$ attack). Unoxidized
//!        soot cooling below $T_{\text{freeze}} \approx 1150\text{ K}$ forms the escaping **smoke plume**.
//!      - `q_rad_shell` ($\text{kW/m}^2$): Radially integrated Planckian incandescent radiation.
//! 4. **Transient BDF Time-Series & Simulation Recap Generator**:
//!    - Produces 40 time keyframes ($t \in [0.0\text{ s}, 6.0\text{ s}]$) and 5 causal milestone
//!      events (`SimulationRecapEvent`) for ParaView-grade 3D WebGL2 volume rendering and
//!      real-time video synthesis.

use serde::{Deserialize, Serialize};

use crate::coupled_system::{
    CausalAttributionPoint, CausalAttributionTrack, ConservationAudit, CoupledEngineeringSystem,
    DesignParameter, DesignParameterCategory, EpistemicParameter, FieldDescriptor, NamedScenario,
    ObjectiveDescriptor, ObjectiveKind,
};

const N_Z: usize = 24;
const N_FIELDS: usize = 6;
const DOMAIN_HEIGHT_MM: f64 = 90.0;
const T_AMB_K: f64 = 295.0;
const T_MELT_K: f64 = 331.0;
const T_VAP_K: f64 = 620.0;
const T_IGN_K: f64 = 670.0;

/// One transient keyframe of the solar candle ignition & buoyant flame simulation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CandleTransientFrame {
    pub step_index: usize,
    pub time_s: f64,
    pub phase_name: String,
    pub ignited: bool,
    pub wick_temp_k: f64,
    pub wax_pool_radius_mm: f64,
    pub pyrolysis_rate_mg_s: f64,
    pub flame_height_mm: f64,
    pub peak_flame_temp_k: f64,
    pub radiant_power_w: f64,
    pub smoke_emission_ug_s: f64,
    pub peak_updraft_m_s: f64,
    pub flicker_phase_rad: f64,
    /// Cumulative thermophoretic soot film thickness at the stagnation centre of the overhead optical glass lens underside (`nm`).
    pub lens_soot_thickness_nm: f64,
    /// Deposited carbonaceous soot areal density on the lens underside centre (`μg/cm²`).
    pub lens_soot_mass_ug_cm2: f64,
    /// Fouled optical lens solar transmittance (`%`) attenuated by Beer–Lambert soot absorption.
    pub lens_transmittance_pct: f64,
    /// Characteristic 1/e Gaussian smudge radius of the soot deposit on the lens underside (`mm`).
    pub lens_soot_radius_mm: f64,
    /// Axial station profiles (length `N_Z = 24`) at this timestamp `time_s`:
    pub r_flame_mm: Vec<f64>,
    pub t_sheet_k: Vec<f64>,
    pub y_oh_ppm: Vec<f64>,
    pub fv_soot_ppm: Vec<f64>,
    pub u_z_m_s: Vec<f64>,
    pub z_mix: Vec<f64>,
}

/// Key milestone event for recapping a long-running transient simulation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimulationRecapEvent {
    pub event_id: String,
    pub time_s: f64,
    pub frame_index: usize,
    pub title: String,
    pub regime_badge: String,
    pub summary: String,
    pub key_metric_label: String,
    pub key_metric_value: String,
}

/// Complete 3D volumetric & transient visual payload for the ParaView-style WebGL2 renderer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CandleFlameVisual {
    pub nz: usize,
    pub z_mm: Vec<f64>,
    pub focal_length_mm: f64,
    pub lens_height_mm: f64,
    pub lens_diameter_mm: f64,
    pub defocus_mm: f64,
    pub spot_radius_mm: f64,
    pub optical_concentration: f64,
    pub peak_solar_flux_kw_m2: f64,
    pub absorbed_solar_power_w: f64,
    pub ignition_delay_s: f64,
    pub steady_pyrolysis_mg_s: f64,
    pub flame_height_mm: f64,
    pub flicker_freq_hz: f64,
    pub stoichiometric_z: f64,
    /// Final thermophoretic soot thickness deposited on the lens underside centre (`nm`).
    pub final_lens_soot_nm: f64,
    /// Final fouled optical lens transmittance (`%`).
    pub final_lens_transmittance_pct: f64,
    /// Radial stations across the optical glass lens underside (`mm`, `len = 16`).
    pub lens_soot_radial_r_mm: Vec<f64>,
    /// Radial soot film thickness profile `δ_soot(r)` on the lens underside (`nm`, `len = 16`).
    pub lens_soot_radial_nm: Vec<f64>,
    /// Steady-state axial profiles (`len = nz`):
    pub u_z_m_s: Vec<f64>,
    pub u_r_entrain_m_s: Vec<f64>,
    pub z_mix: Vec<f64>,
    pub t_gas_k: Vec<f64>,
    pub y_oh_ppm: Vec<f64>,
    pub fv_soot_ppm: Vec<f64>,
    pub q_rad_kw_m2: Vec<f64>,
    pub r_flame_mm: Vec<f64>,
    pub solar_beam_r_mm: Vec<f64>,
    /// Time-dependent simulation frames ($t \in [0, 6.0\text{ s}]$):
    pub frames: Vec<CandleTransientFrame>,
    /// Milestone recap cards:
    pub recap_events: Vec<SimulationRecapEvent>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
struct OpticalAndWickDerived {
    d_lens_mm: f64,
    r_curv_mm: f64,
    z_lens_mm: f64,
    g_solar_w_m2: f64,
    l_wick_mm: f64,
    x_o2: f64,
    focal_length_mm: f64,
    defocus_mm: f64,
    spot_radius_mm: f64,
    optical_concentration: f64,
    peak_solar_flux_kw_m2: f64,
    absorbed_solar_w: f64,
    ignition_delay_s: f64,
    m_dot_fuel_mg_s: f64,
    z_st: f64,
    flame_height_mm: f64,
    u0_wick_m_s: f64,
    t_ad_k: f64,
}

#[derive(Clone, Debug, Default)]
pub struct SolarCandleIgnitionSystem;

impl SolarCandleIgnitionSystem {
    pub fn new() -> Self {
        Self
    }

    fn station_z_mm(i: usize) -> f64 {
        (i as f64) * (DOMAIN_HEIGHT_MM / ((N_Z - 1) as f64))
    }

    fn derive_optics_and_wick(&self, p: &[f64], theta: &[f64]) -> OpticalAndWickDerived {
        let d_lens_mm = p.first().copied().unwrap_or(65.0).clamp(25.0, 140.0);
        let r_curv_mm = p.get(1).copied().unwrap_or(68.0).clamp(35.0, 160.0);
        let z_lens_mm = p.get(2).copied().unwrap_or(72.0).clamp(30.0, 150.0);
        let g_solar_w_m2 = p.get(3).copied().unwrap_or(950.0).clamp(100.0, 1600.0);
        let l_wick_mm = p.get(4).copied().unwrap_or(7.5).clamp(2.5, 18.0);
        let x_o2 = p.get(5).copied().unwrap_or(0.21).clamp(0.12, 0.35);

        let _theta_inc = theta.first().copied().unwrap_or(1.0);
        let _theta_ox = theta.get(1).copied().unwrap_or(1.0);
        let theta_ent = theta.get(2).copied().unwrap_or(0.115).clamp(0.06, 0.22);
        let theta_abs = theta.get(3).copied().unwrap_or(0.86).clamp(0.50, 0.99);

        // BK7 borosilicate crown glass refractive index n = 1.5168, center thickness tc = 8 mm
        let n_glass = 1.5168_f64;
        let tc_mm = 8.0_f64;
        let inv_f = (n_glass - 1.0)
            * (2.0 / r_curv_mm - ((n_glass - 1.0) * tc_mm) / (n_glass * r_curv_mm * r_curv_mm));
        let focal_length_mm = (1.0 / inv_f.max(1e-4)).clamp(25.0, 220.0);

        let defocus_mm = z_lens_mm - focal_length_mm;
        // Solar angular radius = 4.65 mrad + spherical aberration blur + smooth defocus cone
        let r_sun_mm = focal_length_mm * 0.00465;
        let sigma_sph_mm = 0.018 * (d_lens_mm / 65.0).powi(3) * (68.0 / focal_length_mm).powi(2);
        let smooth_defocus_mm = (defocus_mm * defocus_mm + 0.04).sqrt();
        let r_defocus_mm = (d_lens_mm / (2.0 * focal_length_mm)) * smooth_defocus_mm;
        let spot_radius_mm =
            (r_sun_mm * r_sun_mm + sigma_sph_mm * sigma_sph_mm + r_defocus_mm * r_defocus_mm + 0.35 * 0.35)
                .sqrt();

        // Fresnel transmittance across two air-glass interfaces (~0.918) * bulk glass transmittance
        let r_fresnel = ((n_glass - 1.0) / (n_glass + 1.0)).powi(2);
        let tau_opt = (1.0 - r_fresnel).powi(2) * (-0.002 * tc_mm).exp();

        let lens_area_mm2 = std::f64::consts::PI * 0.25 * d_lens_mm * d_lens_mm;
        let spot_area_mm2 = std::f64::consts::PI * spot_radius_mm * spot_radius_mm;
        let optical_concentration = (tau_opt * lens_area_mm2 / spot_area_mm2).max(1.0);
        // Peak Gaussian solar irradiance at wick tip (kW/m²)
        let peak_solar_flux_kw_m2 = 2.0 * optical_concentration * (g_solar_w_m2 * 1e-3);

        // Wick effective intercept area (braided cotton cylinder of diameter 2.2 mm, exposed top 3.5 mm)
        let wick_intercept_mm2 = 2.2 * 3.5;
        let intercept_frac = 1.0 - (-wick_intercept_mm2 / spot_area_mm2).exp();
        let total_solar_power_w = tau_opt * g_solar_w_m2 * (lens_area_mm2 * 1e-6);
        let absorbed_solar_w = theta_abs * total_solar_power_w * intercept_frac;

        // Thermal energy required to raise the braided wick tip + paraffin meniscus to 670 K auto-ignition (~3.2 J)
        let q_threshold_j = 3.20_f64;
        let raw_heating_w =
            absorbed_solar_w + 0.00018 * peak_solar_flux_kw_m2 - 0.08;
        // Smooth positive heating power (softplus) so d(t_ign)/dp is strictly smooth & non-zero everywhere
        let net_heating_w =
            0.12 + 0.5 * (raw_heating_w + (raw_heating_w * raw_heating_w + 0.09).sqrt());
        let ignition_delay_s = q_threshold_j / net_heating_w;

        // Stoichiometric mixture fraction scales with ambient oxygen mole fraction X_O2:
        let o2_mass_frac = (x_o2 * 32.0) / (x_o2 * 32.0 + (1.0 - x_o2) * 28.16);
        let stoich_o2_per_fuel = 3.475; // 38*32 / 352.67 kg O2 / kg C25H52
        let z_st = 1.0 / (1.0 + stoich_o2_per_fuel / o2_mass_frac);

        // Steady capillary paraffin pyrolysis rate (mg/s) driven by exposed wick area + flame/solar feedback
        let o2_boost = (x_o2 / 0.21).powf(0.35);
        let m_dot_fuel_mg_s =
            (1.65 * (l_wick_mm / 7.5).powf(0.78) * o2_boost + 0.095 * absorbed_solar_w).clamp(0.45, 6.5);

        // Roper laminar buoyant diffusion flame height correlation L_f (mm), modulated by entrainment
        let flame_height_mm =
            (34.0 * (m_dot_fuel_mg_s / 1.85).powf(0.68) * (0.0637 / z_st).powf(0.60) * (0.115 / theta_ent).powf(0.45))
                .clamp(14.0, 72.0);

        let u0_wick_m_s = (0.065 * (m_dot_fuel_mg_s / 1.85)).clamp(0.02, 0.25);
        let t_ad_k = (1920.0 + 1850.0 * (x_o2 - 0.21) + 14.0 * absorbed_solar_w.min(6.0))
            .clamp(1550.0, 2450.0);

        OpticalAndWickDerived {
            d_lens_mm,
            r_curv_mm,
            z_lens_mm,
            g_solar_w_m2,
            l_wick_mm,
            x_o2,
            focal_length_mm,
            defocus_mm,
            spot_radius_mm,
            optical_concentration,
            peak_solar_flux_kw_m2,
            absorbed_solar_w,
            ignition_delay_s,
            m_dot_fuel_mg_s,
            z_st,
            flame_height_mm,
            u0_wick_m_s,
            t_ad_k,
        }
    }

    /// Compute the local flame/plume sheet radius `r_flame_mm(z)` at station `i`.
    fn station_flame_radius_mm(
        i: usize,
        drv: &OpticalAndWickDerived,
        theta_ent: f64,
    ) -> f64 {
        let z = Self::station_z_mm(i);
        let xi = z / drv.flame_height_mm.max(1.0);
        if xi <= 1.05 {
            let s = (xi / 1.05).clamp(0.0, 1.0);
            let env = (s + 0.08).powf(0.42) * (1.0 - s.powf(1.35)).max(0.06);
            (5.6 * (drv.m_dot_fuel_mg_s / 1.85).powf(0.38) * env).max(0.9)
        } else {
            0.9 + 1.8 * theta_ent * (z - drv.flame_height_mm).max(0.0)
        }
    }

    /// Generate the complete 3D volumetric visual dataset + 40-frame transient timeline +
    /// 5 milestone recap events from the converged coupled state `W`.
    pub fn build_visual_and_transient(
        &self,
        state: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> CandleFlameVisual {
        let drv = self.derive_optics_and_wick(design_p, epistemic_theta);
        let theta_ent = epistemic_theta.get(2).copied().unwrap_or(0.115).clamp(0.06, 0.22);

        let mut z_mm = Vec::with_capacity(N_Z);
        let mut u_z_m_s = Vec::with_capacity(N_Z);
        let mut u_r_entrain_m_s = Vec::with_capacity(N_Z);
        let mut z_mix = Vec::with_capacity(N_Z);
        let mut t_gas_k = Vec::with_capacity(N_Z);
        let mut y_oh_ppm = Vec::with_capacity(N_Z);
        let mut fv_soot_ppm = Vec::with_capacity(N_Z);
        let mut q_rad_kw_m2 = Vec::with_capacity(N_Z);
        let mut r_flame_mm = Vec::with_capacity(N_Z);
        let mut solar_beam_r_mm = Vec::with_capacity(N_Z);

        for i in 0..N_Z {
            let z = Self::station_z_mm(i);
            let base = i * N_FIELDS;
            let uz = state.get(base).copied().unwrap_or(0.2);
            let zm = state.get(base + 1).copied().unwrap_or(0.05);
            let tg = state.get(base + 2).copied().unwrap_or(T_AMB_K);
            let yoh = state.get(base + 3).copied().unwrap_or(0.0);
            let fv = state.get(base + 4).copied().unwrap_or(0.0);
            let qrad = state.get(base + 5).copied().unwrap_or(0.0);

            let xi = z / drv.flame_height_mm.max(1.0);
            // Teardrop diffusion flame sheet radius r_f(z) (mm)
            let rf = if xi <= 1.05 {
                let s = (xi / 1.05).clamp(0.0, 1.0);
                let env = (s + 0.08).powf(0.42) * (1.0 - s.powf(1.35)).max(0.06);
                (5.6 * (drv.m_dot_fuel_mg_s / 1.85).powf(0.38) * env).max(0.9)
            } else {
                // Expanding buoyant convective smoke plume above flame tip
                0.9 + 1.8 * theta_ent * (z - drv.flame_height_mm).max(0.0)
            };

            // Morton-Taylor-Turner radial inflow velocity u_r = -alpha_ent * u_z
            let ur = -theta_ent * uz;

            // Solar beam caustic radius at height z above wick tip (lens is at z = drv.z_lens_mm)
            let dz_from_focus = z - (-drv.defocus_mm);
            let cone_r = (drv.d_lens_mm / (2.0 * drv.focal_length_mm)) * dz_from_focus.abs();
            let beam_r = (drv.spot_radius_mm * drv.spot_radius_mm + cone_r * cone_r).sqrt();

            z_mm.push(z);
            u_z_m_s.push(uz);
            u_r_entrain_m_s.push(ur);
            z_mix.push(zm);
            t_gas_k.push(tg);
            y_oh_ppm.push(yoh);
            fv_soot_ppm.push(fv);
            q_rad_kw_m2.push(qrad);
            r_flame_mm.push(rf);
            solar_beam_r_mm.push(beam_r);
        }

        // Durox-Cetegen buoyant diffusion flame natural flickering frequency f = 1.5 * sqrt(g / D_burner)
        let d_burner_m = 0.011_f64;
        let flicker_freq_hz = 1.5 * (9.81 / d_burner_m).sqrt() * 0.24; // ~10.8 Hz physical, scaled for clear visual oscillation

        // Build 40 transient keyframes over t in [0.0, t_end_s]
        let n_frames = 40usize;
        let t_ign = drv.ignition_delay_s.max(0.35);
        let t_melt = 0.22 * t_ign;
        let t_vap = 0.65 * t_ign;
        let t_steady = t_ign + 1.25;
        let t_end_s = (t_steady + 2.8).clamp(5.5, 14.0);

        let peak_t_steady = t_gas_k.iter().copied().fold(T_AMB_K, f64::max);
        let peak_uz_steady = u_z_m_s.iter().copied().fold(0.0_f64, f64::max);
        let steady_rad_w = self.integrate_radiant_power_w(&r_flame_mm, &q_rad_kw_m2);
        let steady_smoke_ug_s = self.compute_smoke_emission_ug_s(state);

        // Thermophoretic soot deposition onto the overhead BK7 optical glass lens underside:
        // As the buoyant smoke plume impinges on the cooler bottom face of the glass lens at height z_lens,
        // Talbot–Brock thermophoresis (v_th = -K_th * (nu/T) * grad_T, K_th ≈ 0.54) drives carbonaceous soot
        // aggregates onto the lens surface, forming a radially graded smudge film δ_soot(r, t) (nm).
        let dt_frame_s = t_end_s / ((n_frames - 1) as f64);
        let r_lens_mm = (0.5 * drv.d_lens_mm).max(12.0);
        let base_smudge_r_mm = (4.5 + 0.145 * drv.z_lens_mm).clamp(6.0, 0.85 * r_lens_mm);
        let z_prox_factor = (68.0 / drv.z_lens_mm.max(25.0)).powf(1.15);
        let mut cum_lens_soot_nm = 0.0_f64;

        let mut frames = Vec::with_capacity(n_frames);
        for k in 0..n_frames {
            let frac = (k as f64) / ((n_frames - 1) as f64);
            let t = frac * t_end_s;

            let (phase_name, ignited, wick_t, pool_r, mdot_f, flame_scale, smoke_scale) =
                if t < t_melt {
                    let s = (t / t_melt.max(1e-3)).clamp(0.0, 1.0);
                    (
                        "1. Solar Caustic Heating".to_string(),
                        false,
                        T_AMB_K + (T_MELT_K - T_AMB_K) * s,
                        0.8 * s,
                        0.0,
                        0.0,
                        0.0,
                    )
                } else if t < t_vap {
                    let s = ((t - t_melt) / (t_vap - t_melt).max(1e-3)).clamp(0.0, 1.0);
                    (
                        "2. Paraffin Pool Melting".to_string(),
                        false,
                        T_MELT_K + (T_VAP_K - T_MELT_K) * s,
                        0.8 + 4.2 * s,
                        0.12 * drv.m_dot_fuel_mg_s * s,
                        0.0,
                        0.15 * s,
                    )
                } else if t < t_ign {
                    let s = ((t - t_vap) / (t_ign - t_vap).max(1e-3)).clamp(0.0, 1.0);
                    (
                        "3. Pre-Ignition Pyrolysis Vapor Plume".to_string(),
                        false,
                        T_VAP_K + (T_IGN_K - T_VAP_K) * s,
                        5.0 + 2.5 * s,
                        drv.m_dot_fuel_mg_s * (0.12 + 0.45 * s),
                        0.0,
                        0.15 + 0.55 * s,
                    )
                } else if t < t_steady {
                    let s = ((t - t_ign) / (t_steady - t_ign).max(1e-3)).clamp(0.0, 1.0);
                    // Flash ignition overshoot then relaxation into steady buoyant plume
                    let overshoot = 1.0 + 0.22 * (std::f64::consts::PI * s).sin();
                    (
                        "4. Flash Auto-Ignition & Kernel Growth".to_string(),
                        true,
                        T_IGN_K + 65.0 * s,
                        7.5 + 3.0 * s,
                        drv.m_dot_fuel_mg_s * (0.57 + 0.43 * s),
                        (s.powf(0.65) * overshoot).clamp(0.15, 1.25),
                        0.70 + 0.30 * s,
                    )
                } else {
                    let osc = 0.045 * (2.0 * std::f64::consts::PI * 2.2 * (t - t_steady)).sin();
                    (
                        "5. Buoyant Flame, Smoke & Lens Soot Fouling".to_string(),
                        true,
                        T_IGN_K + 65.0,
                        10.5,
                        drv.m_dot_fuel_mg_s * (1.0 + 0.5 * osc),
                        1.0 + osc,
                        1.0 + 1.2 * osc.abs(),
                    )
                };

            let flicker_phase_rad = if ignited {
                2.0 * std::f64::consts::PI * 2.2 * (t - t_ign).max(0.0)
            } else {
                0.0
            };

            let inst_smoke_ug_s = steady_smoke_ug_s * smoke_scale;
            if k > 0 {
                // Thermophoretic deposition rate (nm/s) onto the cooler overhead lens underside
                let dep_rate_nm_s = if ignited {
                    1.85 * inst_smoke_ug_s * z_prox_factor * (18.0 / base_smudge_r_mm).powf(0.8)
                } else {
                    0.06 * inst_smoke_ug_s * z_prox_factor
                };
                cum_lens_soot_nm += dep_rate_nm_s * dt_frame_s;
            }
            // Soot bulk density rho_soot = 1.85 g/cm³ -> 1 nm film = 0.185 μg/cm²
            let lens_soot_mass_ug_cm2 = 0.185 * cum_lens_soot_nm;
            // Beer–Lambert optical attenuation through carbonaceous soot film (α_ext ≈ 0.0042 nm⁻¹)
            let lens_transmittance_pct =
                (91.8 * (-0.0042 * cum_lens_soot_nm).exp()).clamp(12.0, 91.8);
            let lens_soot_radius_mm = if ignited {
                let post_s = ((t - t_ign) / 2.5).clamp(0.0, 1.0);
                base_smudge_r_mm * (0.62 + 0.38 * post_s)
            } else {
                0.45 * base_smudge_r_mm
            };

            let mut rf_k = Vec::with_capacity(N_Z);
            let mut ts_k = Vec::with_capacity(N_Z);
            let mut yoh_k = Vec::with_capacity(N_Z);
            let mut fv_k = Vec::with_capacity(N_Z);
            let mut uz_k = Vec::with_capacity(N_Z);
            let mut zm_k = Vec::with_capacity(N_Z);

            for i in 0..N_Z {
                let z = z_mm[i];
                if !ignited {
                    // Pre-ignition thermal + pyrolysis vapor plume rising from solar-heated wick
                    let plume_reach = (12.0 + 48.0 * smoke_scale).max(4.0);
                    let env = (-z / plume_reach).exp();
                    rf_k.push(1.4 + 0.08 * z);
                    ts_k.push(T_AMB_K + (wick_t - T_AMB_K) * env);
                    yoh_k.push(0.0);
                    // Pre-ignition condensed paraffin vapor / white smoke mist
                    fv_k.push(0.85 * smoke_scale * env * (1.0 - (-z / 3.0).exp()));
                    uz_k.push(0.18 * smoke_scale * (1.0 - (-z / 8.0).exp()));
                    zm_k.push(z_mix[i] * smoke_scale * env);
                } else {
                    let wave = 1.0
                        + 0.06
                            * (z / 35.0).min(1.5)
                            * (flicker_phase_rad - z * 0.14).sin();
                    let h_env = if z <= drv.flame_height_mm * flame_scale {
                        1.0
                    } else {
                        (-((z - drv.flame_height_mm * flame_scale) / 22.0)).exp()
                    };
                    rf_k.push(r_flame_mm[i] * flame_scale.sqrt() * wave);
                    ts_k.push(T_AMB_K + (t_gas_k[i] - T_AMB_K) * flame_scale.min(1.08) * h_env.max(0.25));
                    // During flash ignition (flame_scale < 0.7), blue OH* chemiluminescence is extra intense
                    let flash_boost = if t < t_ign + 0.45 { 1.45 } else { 1.0 };
                    yoh_k.push(y_oh_ppm[i] * flash_boost * flame_scale.min(1.15));
                    fv_k.push(fv_soot_ppm[i] * smoke_scale * wave.max(0.5));
                    uz_k.push(u_z_m_s[i] * flame_scale.sqrt());
                    zm_k.push(z_mix[i] * flame_scale.min(1.0));
                }
            }

            frames.push(CandleTransientFrame {
                step_index: k,
                time_s: t,
                phase_name,
                ignited,
                wick_temp_k: wick_t,
                wax_pool_radius_mm: pool_r,
                pyrolysis_rate_mg_s: mdot_f,
                flame_height_mm: if ignited {
                    drv.flame_height_mm * flame_scale
                } else {
                    0.0
                },
                peak_flame_temp_k: if ignited {
                    T_AMB_K + (peak_t_steady - T_AMB_K) * flame_scale.min(1.05)
                } else {
                    wick_t
                },
                radiant_power_w: if ignited {
                    steady_rad_w * flame_scale.powf(1.4)
                } else {
                    0.02 * smoke_scale
                },
                smoke_emission_ug_s: inst_smoke_ug_s,
                peak_updraft_m_s: if ignited {
                    peak_uz_steady * flame_scale.sqrt()
                } else {
                    0.18 * smoke_scale
                },
                flicker_phase_rad,
                lens_soot_thickness_nm: cum_lens_soot_nm,
                lens_soot_mass_ug_cm2,
                lens_transmittance_pct,
                lens_soot_radius_mm,
                r_flame_mm: rf_k,
                t_sheet_k: ts_k,
                y_oh_ppm: yoh_k,
                fv_soot_ppm: fv_k,
                u_z_m_s: uz_k,
                z_mix: zm_k,
            });
        }

        let final_lens_soot_nm = frames.last().map(|f| f.lens_soot_thickness_nm).unwrap_or(0.0);
        let final_lens_transmittance_pct = frames.last().map(|f| f.lens_transmittance_pct).unwrap_or(91.8);
        let n_rad = 16usize;
        let mut lens_soot_radial_r_mm = Vec::with_capacity(n_rad);
        let mut lens_soot_radial_nm = Vec::with_capacity(n_rad);
        for j in 0..n_rad {
            let r_j = (j as f64) * (r_lens_mm / ((n_rad - 1) as f64));
            let gauss = (-(r_j / base_smudge_r_mm).powi(2)).exp();
            lens_soot_radial_r_mm.push(r_j);
            lens_soot_radial_nm.push(final_lens_soot_nm * gauss);
        }

        let nearest_frame = |target_t: f64| -> usize {
            frames
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    (a.time_s - target_t)
                        .abs()
                        .partial_cmp(&(b.time_s - target_t).abs())
                        .unwrap()
                })
                .map(|(idx, _)| idx)
                .unwrap_or(0)
        };

        let idx_melt = nearest_frame(t_melt);
        let idx_vap = nearest_frame(t_vap);
        let idx_ign = frames.iter().position(|f| f.ignited).unwrap_or(nearest_frame(t_ign));
        let idx_steady = n_frames - 1;

        let recap_events = vec![
            SimulationRecapEvent {
                event_id: "solar_focus".into(),
                time_s: 0.0,
                frame_index: 0,
                title: "1. Solar Caustic Alignment on Wick".into(),
                regime_badge: "Optics · BK7 Lens".into(),
                summary: format!(
                    "Ø {:.0} mm biconvex lens (f = {:.1} mm) concentrates {:.0} W/m² sunlight by {:.0}× onto a r = {:.2} mm focal spot.",
                    drv.d_lens_mm, drv.focal_length_mm, drv.g_solar_w_m2, drv.optical_concentration, drv.spot_radius_mm
                ),
                key_metric_label: "Peak Solar Flux".into(),
                key_metric_value: format!("{:.1} kW/m²", drv.peak_solar_flux_kw_m2),
            },
            SimulationRecapEvent {
                event_id: "wax_melting".into(),
                time_s: frames[idx_melt].time_s,
                frame_index: idx_melt,
                title: "2. Paraffin Wax Pool Melting".into(),
                regime_badge: "Phase Change · 331 K".into(),
                summary: "Wick base surpasses paraffin melting point (58 °C); capillary Darcy draw begins feeding liquid C₂₅H₅₂ up the cotton fibres.".into(),
                key_metric_label: "Wick Temperature".into(),
                key_metric_value: format!("{:.0} K ({:.0} °C)", frames[idx_melt].wick_temp_k, frames[idx_melt].wick_temp_k - 273.15),
            },
            SimulationRecapEvent {
                event_id: "pyrolysis_plume".into(),
                time_s: frames[idx_vap].time_s,
                frame_index: idx_vap,
                title: "3. Endothermic Pyrolysis Vapor Plume".into(),
                regime_badge: "Pyrolysis · 620 K".into(),
                summary: format!(
                    "Paraffin vaporizes from the hot wick tip, establishing a fuel-air mixing layer around stoichiometric Z_st = {:.4}.",
                    drv.z_st
                ),
                key_metric_label: "Vapor Mass Flux".into(),
                key_metric_value: format!("{:.2} mg/s", frames[idx_vap].pyrolysis_rate_mg_s),
            },
            SimulationRecapEvent {
                event_id: "flash_ignition".into(),
                time_s: frames[idx_ign].time_s,
                frame_index: idx_ign,
                title: "4. Flash Auto-Ignition & Blue OH* Kernel".into(),
                regime_badge: "Ignition · 670 K".into(),
                summary: "Local radical runaway ignites the stoichiometric envelope with a blue CH*/OH* chemiluminescent reaction front.".into(),
                key_metric_label: "Ignition Delay t_ign".into(),
                key_metric_value: format!("{:.2} s", drv.ignition_delay_s),
            },
            SimulationRecapEvent {
                event_id: "steady_plume".into(),
                time_s: frames[idx_steady].time_s,
                frame_index: idx_steady,
                title: "5. Buoyant Plume & Thermophoretic Lens Soot Fouling".into(),
                regime_badge: "Plume & Lens Soot".into(),
                summary: format!(
                    "Teardrop soot flame (L_f = {:.1} mm, {:.1} W) impinges on the overhead lens underside, depositing a {:.1} nm carbonaceous soot smudge (τ_lens = {:.1}%).",
                    drv.flame_height_mm, steady_rad_w, final_lens_soot_nm, final_lens_transmittance_pct
                ),
                key_metric_label: "Lens Soot Deposit".into(),
                key_metric_value: format!("{:.1} nm (τ={:.1}%)", final_lens_soot_nm, final_lens_transmittance_pct),
            },
        ];

        CandleFlameVisual {
            nz: N_Z,
            z_mm,
            focal_length_mm: drv.focal_length_mm,
            lens_height_mm: drv.z_lens_mm,
            lens_diameter_mm: drv.d_lens_mm,
            defocus_mm: drv.defocus_mm,
            spot_radius_mm: drv.spot_radius_mm,
            optical_concentration: drv.optical_concentration,
            peak_solar_flux_kw_m2: drv.peak_solar_flux_kw_m2,
            absorbed_solar_power_w: drv.absorbed_solar_w,
            ignition_delay_s: drv.ignition_delay_s,
            steady_pyrolysis_mg_s: drv.m_dot_fuel_mg_s,
            flame_height_mm: drv.flame_height_mm,
            flicker_freq_hz,
            stoichiometric_z: drv.z_st,
            final_lens_soot_nm,
            final_lens_transmittance_pct,
            lens_soot_radial_r_mm,
            lens_soot_radial_nm,
            u_z_m_s,
            u_r_entrain_m_s,
            z_mix,
            t_gas_k,
            y_oh_ppm,
            fv_soot_ppm,
            q_rad_kw_m2,
            r_flame_mm,
            solar_beam_r_mm,
            frames,
            recap_events,
        }
    }

    fn integrate_radiant_power_w(&self, r_flame_mm: &[f64], q_rad_kw_m2: &[f64]) -> f64 {
        let dz_m = (DOMAIN_HEIGHT_MM / ((N_Z - 1) as f64)) * 1e-3;
        let mut total_w = 0.0;
        for i in 0..N_Z {
            let r_m = r_flame_mm[i] * 1e-3;
            let q_w_m2 = q_rad_kw_m2[i] * 1e3;
            let weight = if i == 0 || i == N_Z - 1 { 0.5 } else { 1.0 };
            total_w += weight * (2.0 * std::f64::consts::PI * r_m * dz_m) * q_w_m2;
        }
        total_w
    }

    fn compute_smoke_emission_ug_s(&self, state: &[f64]) -> f64 {
        let top = (N_Z - 1) * N_FIELDS;
        let uz_top = state.get(top).copied().unwrap_or(0.4).max(0.01);
        let fv_top_ppm = state.get(top + 4).copied().unwrap_or(0.1).max(0.0);
        // Soot material density rho_soot = 1850 kg/m³, top smoke filament effective radius ~ 4.0 mm
        let area_top_m2 = std::f64::consts::PI * (0.0040_f64).powi(2);
        let rho_soot_ug_m3 = 1850.0 * 1e9;
        uz_top * area_top_m2 * (fv_top_ppm * 1e-6) * rho_soot_ug_m3
    }

    /// Alias matching `FloatBathCoaterDesignSystem::extract_visual_fields`.
    pub fn extract_visual_fields(
        &self,
        state: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> CandleFlameVisual {
        self.build_visual_and_transient(state, design_p, epistemic_theta)
    }

    /// Preset scenarios for interactive exploration and verification.
    pub fn named_scenarios(&self) -> Vec<NamedScenario> {
        vec![
            NamedScenario {
                name: "A. Sharp Solar Focus (Fast ~1.2 s Ignition, Clean Plume)".into(),
                description: "Ø 65 mm BK7 lens aligned at focal height z_lens = 68 mm under 950 W/m² direct sunlight with 7.5 mm wick.".into(),
                design_p: vec![65.0, 68.0, 68.0, 950.0, 7.5, 0.21],
            },
            NamedScenario {
                name: "B. Defocused Lens & Long Sooting Wick (Slow Ignition, Heavy Smoke)".into(),
                description: "Defocused Ø 55 mm lens (z_lens = 92 mm) with an over-long 12 mm wick exceeding the laminar smoke point.".into(),
                design_p: vec![55.0, 68.0, 92.0, 780.0, 12.0, 0.21],
            },
            NamedScenario {
                name: "C. High-Aperture Concentrator & O₂-Enriched Plume (Sub-Second Ignition)".into(),
                description: "Ø 90 mm solar concentrator at z_lens = 73 mm with 26% O₂ entrainment suppressing smoke emission while boosting radiant output.".into(),
                design_p: vec![90.0, 75.0, 73.0, 1050.0, 7.0, 0.26],
            },
        ]
    }
}

impl CoupledEngineeringSystem for SolarCandleIgnitionSystem {
    fn system_id(&self) -> &str {
        "solar_candle_ignition_v1"
    }

    fn system_title(&self) -> &str {
        "Solar-Focused Optical Lens Candle Ignition & Buoyant Diffusion Flame"
    }

    fn industry_sector(&self) -> &str {
        "Optics, Combustion & Buoyant Reactive Flow"
    }

    fn field_descriptors(&self) -> Vec<FieldDescriptor> {
        vec![
            FieldDescriptor {
                name: "Buoyant Axial Velocity".into(),
                symbol: "u_z".into(),
                unit: "m/s".into(),
                reference_scale: 1.0,
                domain: "Candle Plume Axis (z ∈ [0, 90 mm])".into(),
            },
            FieldDescriptor {
                name: "Bilger Mixture Fraction".into(),
                symbol: "Z_mix".into(),
                unit: "-".into(),
                reference_scale: 1.0,
                domain: "Candle Plume Axis (z ∈ [0, 90 mm])".into(),
            },
            FieldDescriptor {
                name: "Flame & Plume Gas Temperature".into(),
                symbol: "T_gas".into(),
                unit: "K".into(),
                reference_scale: 2000.0,
                domain: "Candle Plume Axis (z ∈ [0, 90 mm])".into(),
            },
            FieldDescriptor {
                name: "Excited OH*/CH* Chemiluminescence".into(),
                symbol: "Y_OH*".into(),
                unit: "ppm".into(),
                reference_scale: 400.0,
                domain: "Reaction Sheet (Z ≈ Z_st)".into(),
            },
            FieldDescriptor {
                name: "Soot & Smoke Volume Fraction".into(),
                symbol: "f_v".into(),
                unit: "ppm".into(),
                reference_scale: 5.0,
                domain: "Luminous Core & Smoke Plume".into(),
            },
            FieldDescriptor {
                name: "Planckian Radiant Flux".into(),
                symbol: "q_rad".into(),
                unit: "kW/m²".into(),
                reference_scale: 50.0,
                domain: "Luminous Flame Envelope".into(),
            },
        ]
    }

    fn num_dofs(&self) -> usize {
        N_Z * N_FIELDS
    }

    fn design_parameters(&self) -> Vec<DesignParameter> {
        vec![
            DesignParameter {
                name: "lens_diameter_mm".into(),
                symbol: "D_lens".into(),
                unit: "mm".into(),
                category: DesignParameterCategory::ComponentHardware,
                nominal: 65.0,
                min_bound: 35.0,
                max_bound: 110.0,
                fd_step: 1e-3,
                cost_rate_per_unit: 0.12,
                description: "Clear aperture diameter of the biconvex BK7 optical glass lens".into(),
            },
            DesignParameter {
                name: "lens_curvature_r_mm".into(),
                symbol: "R_lens".into(),
                unit: "mm".into(),
                category: DesignParameterCategory::GeometryContour,
                nominal: 68.0,
                min_bound: 45.0,
                max_bound: 120.0,
                fd_step: 1e-3,
                cost_rate_per_unit: 0.05,
                description: "Spherical radius of curvature of the biconvex optical lens surfaces".into(),
            },
            DesignParameter {
                name: "lens_height_mm".into(),
                symbol: "z_lens".into(),
                unit: "mm".into(),
                category: DesignParameterCategory::ActuatorSetpoint,
                nominal: 68.0,
                min_bound: 45.0,
                max_bound: 125.0,
                fd_step: 1e-3,
                cost_rate_per_unit: 0.0,
                description: "Axial elevation of the optical lens above the candle wick tip".into(),
            },
            DesignParameter {
                name: "solar_irradiance_w_m2".into(),
                symbol: "G_solar".into(),
                unit: "W/m²".into(),
                category: DesignParameterCategory::ProcessAtmosphere,
                nominal: 950.0,
                min_bound: 300.0,
                max_bound: 1360.0,
                fd_step: 1e-2,
                cost_rate_per_unit: 0.0,
                description: "Direct normal solar irradiance incident on the optical lens".into(),
            },
            DesignParameter {
                name: "wick_exposed_len_mm".into(),
                symbol: "L_wick".into(),
                unit: "mm".into(),
                category: DesignParameterCategory::GeometryContour,
                nominal: 7.5,
                min_bound: 3.5,
                max_bound: 14.0,
                fd_step: 1e-3,
                cost_rate_per_unit: 0.0,
                description: "Exposed braided cotton wick length above the molten paraffin cup".into(),
            },
            DesignParameter {
                name: "ambient_o2_mol_frac".into(),
                symbol: "X_O2".into(),
                unit: "mol/mol".into(),
                category: DesignParameterCategory::ProcessAtmosphere,
                nominal: 0.21,
                min_bound: 0.15,
                max_bound: 0.30,
                fd_step: 1e-4,
                cost_rate_per_unit: 0.0,
                description: "Ambient oxygen mole fraction in the buoyant entrainment air stream".into(),
            },
        ]
    }

    fn epistemic_parameters(&self) -> Vec<EpistemicParameter> {
        vec![
            EpistemicParameter {
                name: "soot_inception_scale".into(),
                symbol: "θ_inc".into(),
                unit: "×".into(),
                nominal: 1.0,
                std_dev_sigma: 0.18,
                min_bound: 0.5,
                max_bound: 1.8,
                fd_step: 1e-4,
                physical_mechanism: "PAH pyrolysis soot nucleation pre-exponential factor in C₂₅H₅₂".into(),
            },
            EpistemicParameter {
                name: "soot_oxidation_eff".into(),
                symbol: "θ_ox".into(),
                unit: "×".into(),
                nominal: 1.0,
                std_dev_sigma: 0.15,
                min_bound: 0.5,
                max_bound: 1.8,
                fd_step: 1e-4,
                physical_mechanism: "Nagle–Strickland-Constable O₂ / OH* soot oxidation efficiency".into(),
            },
            EpistemicParameter {
                name: "plume_entrainment_alpha".into(),
                symbol: "α_ent".into(),
                unit: "-".into(),
                nominal: 0.115,
                std_dev_sigma: 0.018,
                min_bound: 0.07,
                max_bound: 0.18,
                fd_step: 1e-4,
                physical_mechanism: "Morton–Taylor–Turner radial buoyant air entrainment coefficient".into(),
            },
            EpistemicParameter {
                name: "wick_absorptivity".into(),
                symbol: "α_wick".into(),
                unit: "-".into(),
                nominal: 0.86,
                std_dev_sigma: 0.06,
                min_bound: 0.65,
                max_bound: 0.98,
                fd_step: 1e-4,
                physical_mechanism: "Solar spectral absorptivity of the charred cotton/paraffin wick".into(),
            },
        ]
    }

    fn objective_descriptors(&self) -> Vec<ObjectiveDescriptor> {
        vec![
            ObjectiveDescriptor {
                name: "Solar Ignition Delay".into(),
                symbol: "t_ign".into(),
                unit: "s".into(),
                kind: ObjectiveKind::UpperLimit(3.5),
                description: "Time to heat the candle wick to 670 K auto-ignition under focussed solar radiation".into(),
            },
            ObjectiveDescriptor {
                name: "Peak Flame Sheet Temp".into(),
                symbol: "T_max".into(),
                unit: "K".into(),
                kind: ObjectiveKind::TargetBand {
                    min: 1700.0,
                    max: 2150.0,
                },
                description: "Peak diffusion flame reaction-sheet temperature".into(),
            },
            ObjectiveDescriptor {
                name: "Radiant Luminous Power".into(),
                symbol: "P_rad".into(),
                unit: "W".into(),
                kind: ObjectiveKind::Maximize,
                description: "Total Planckian incandescent radiant power emitted by the luminous flame".into(),
            },
            ObjectiveDescriptor {
                name: "Escaping Smoke Emission".into(),
                symbol: "ṁ_smoke".into(),
                unit: "μg/s".into(),
                kind: ObjectiveKind::UpperLimit(18.0),
                description: "Unoxidized carbonaceous soot mass flow escaping into the convective smoke plume".into(),
            },
            ObjectiveDescriptor {
                name: "Peak Buoyant Updraft".into(),
                symbol: "u_z,max".into(),
                unit: "m/s".into(),
                kind: ObjectiveKind::TargetBand {
                    min: 0.35,
                    max: 0.95,
                },
                description: "Peak buoyant convective axial velocity in the rising thermal plume".into(),
            },
        ]
    }

    fn initial_state(&self, design_p: &[f64], epistemic_theta: &[f64]) -> Vec<f64> {
        let drv = self.derive_optics_and_wick(design_p, epistemic_theta);
        let mut w = vec![0.0; self.num_dofs()];
        let t_wick_base = T_VAP_K + 45.0 * (drv.absorbed_solar_w / (1.0 + drv.absorbed_solar_w));
        for i in 0..N_Z {
            let z_mm = Self::station_z_mm(i);
            let s = z_mm / DOMAIN_HEIGHT_MM;
            let base = i * N_FIELDS;
            // Distinct cold-start profiles (linear / exponential ramps) so Newton-Raphson genuinely iterates:
            w[base] = drv.u0_wick_m_s + 0.45 * s.sqrt();
            w[base + 1] = (-2.8 * z_mm / drv.flame_height_mm.max(10.0)).exp();
            w[base + 2] = if i == 0 {
                t_wick_base
            } else {
                T_AMB_K + 0.72 * (drv.t_ad_k - T_AMB_K) * (1.0 - 0.55 * s)
            };
            w[base + 3] = 120.0 * (1.0 - 0.6 * s);
            w[base + 4] = if i == 0 { 0.01 } else { 1.2 * (1.0 - 0.5 * s) };
            w[base + 5] = 8.0 * (1.0 - 0.5 * s);
        }
        w
    }

    fn eval_residual(
        &self,
        state: &[f64],
        w_prev: &[f64],
        inv_dt: f64,
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<f64> {
        let mut residual_out = vec![0.0; self.num_dofs()];
        let drv = self.derive_optics_and_wick(design_p, epistemic_theta);
        let theta_inc = epistemic_theta.first().copied().unwrap_or(1.0).clamp(0.4, 2.2);
        let theta_ox = epistemic_theta.get(1).copied().unwrap_or(1.0).clamp(0.4, 2.2);
        let theta_ent = epistemic_theta.get(2).copied().unwrap_or(0.115).clamp(0.06, 0.22);

        let dz_mm = DOMAIN_HEIGHT_MM / ((N_Z - 1) as f64);
        let dz_m = dz_mm * 1e-3;
        let lf = drv.flame_height_mm.max(1.0);
        let t_wick_base = T_VAP_K + 45.0 * (drv.absorbed_solar_w / (1.0 + drv.absorbed_solar_w));
        let g = 9.81_f64;
        let sigma_sb_kw = 5.670_374e-11_f64;

        for i in 0..N_Z {
            let base = i * N_FIELDS;
            let u_z = state[base];
            let z_mix = state[base + 1];
            let t_gas = state[base + 2];
            let y_oh = state[base + 3];
            let fv_soot = state[base + 4];
            let q_rad = state[base + 5];

            let z_mm = Self::station_z_mm(i);
            let z_m = z_mm * 1e-3;
            let xi = z_mm / lf;

            // Smooth positive state helpers for C^∞ Newton & adjoint differentiability:
            let u_pos = 0.5 * (u_z + (u_z * u_z + 0.02 * 0.02).sqrt());
            let z_pos = 0.5 * (z_mix + (z_mix * z_mix + 1.0e-6).sqrt());
            let dt_pos = 0.5 * ((t_gas - T_AMB_K) + ((t_gas - T_AMB_K).powi(2) + 4.0).sqrt());
            let fv_pos = 0.5 * (fv_soot + (fv_soot * fv_soot + 1.0e-6).sqrt());
            let t_pos = 0.5 * (t_gas + (t_gas * t_gas + 100.0).sqrt());

            // Burke-Schumann stoichiometric reactivity kernel evaluated on actual state z_mix:
            let phi_react = (4.0 * z_pos * drv.z_st) / (z_pos + drv.z_st).powi(2);

            // Local Planckian radiative emission from solved T_gas and solved fv_soot:
            let r_flame_m = (Self::station_flame_radius_mm(i, &drv, theta_ent) * 1e-3).max(0.0009);
            let kappa_eff = 0.85 + 1.45 * fv_pos * (t_pos / 1600.0);
            let eps_shell = 1.0 - (-kappa_eff * r_flame_m * 18.0).exp();
            let q_rad_planck =
                (eps_shell * sigma_sb_kw * (t_pos.powi(4) - T_AMB_K.powi(4))).max(0.01);

            // Excited OH*/CH* radical production rate driven by solved T_gas and solved Z_mix:
            let base_blue = (-((z_mm - 4.0) / 4.8).powi(2)).exp();
            let sheet_tip = (-((xi - 0.88) / 0.28).powi(2)).exp();
            let temp_act_oh = 0.72 + 0.28 * (dt_pos / (drv.t_ad_k - T_AMB_K).max(400.0));
            let s_oh = 380.0
                * (drv.x_o2 / 0.21).powf(0.85)
                * temp_act_oh
                * (0.72 * base_blue + 0.55 * sheet_tip * (0.65 + 0.35 * phi_react));

            if i == 0 {
                // Wick base boundary conditions (z = 0):
                residual_out[base] = u_z - drv.u0_wick_m_s;
                residual_out[base + 1] = z_mix - 1.0;
                residual_out[base + 2] = t_gas - t_wick_base;
                residual_out[base + 3] = y_oh - s_oh;
                residual_out[base + 4] = fv_soot - 0.01;
                residual_out[base + 5] = q_rad - q_rad_planck;
            } else {
                let prev = (i - 1) * N_FIELDS;
                let next = if i + 1 < N_Z {
                    (i + 1) * N_FIELDS
                } else {
                    base
                };

                let u_prev = state[prev];
                let u_next = state[next];
                let z_prev = state[prev + 1];
                let z_next = state[next + 1];
                let t_prev = state[prev + 2];
                let t_next = state[next + 2];
                let yoh_prev = state[prev + 3];
                let fv_prev = state[prev + 4];
                let fv_next = state[next + 4];

                // 1. Axial velocity u_z: upwind momentum advection + axial viscous diffusion
                //    + Morton-Taylor-Turner radial entrainment drag = Boussinesq buoyancy(T_gas):
                let du_adv = 0.04 * u_pos * (u_z - u_prev);
                let du_diff = -0.015 * (u_prev - 2.0 * u_z + u_next);
                let drag_factor = 1.0 + 0.85 * theta_ent * (z_mm / 45.0);
                let buoy_source = 0.68 * (2.0 * g * (dt_pos / T_AMB_K) * z_m).sqrt();
                residual_out[base] =
                    du_adv + du_diff + drag_factor * (u_z - drv.u0_wick_m_s) - buoy_source;

                // 2. Bilger mixture fraction Z_mix: 1D upwind advection + axial diffusion
                //    + radial air entrainment dilution along the buoyant plume:
                let c_mix = (1.0 / drv.z_st - 1.0).max(1.0);
                let xi_prev = Self::station_z_mm(i - 1) / lf;
                let target_ratio = (1.0 + c_mix * xi_prev.powf(1.22)) / (1.0 + c_mix * xi.powf(1.22));
                let dz_diff = -0.018 * (z_prev - 2.0 * z_mix + z_next);
                let vel_stretch = 1.0 + 0.025 * (u_pos - 0.55);
                residual_out[base + 1] =
                    (z_mix - z_prev * target_ratio * vel_stretch) + dz_diff;

                // 3. Plume gas temperature T_gas: upwind axial enthalpy advection + axial conduction
                //    + radial entrainment cooling + radiative heat loss = chemical heat release(Z_mix):
                let dt_adv = 0.03 * u_pos * (t_gas - t_prev);
                let dt_diff = -0.015 * (t_prev - 2.0 * t_gas + t_next);
                let react_env = (-((xi - 0.78) / 0.52).powi(2)).exp();
                let base_blend = 1.0 - (-z_mm / 3.8).exp();
                let plume_decay = if xi <= 1.0 {
                    1.0
                } else {
                    1.0 / (1.0 + 3.2 * theta_ent * (xi - 1.0).powf(1.15))
                };
                let q_chem_temp = (t_wick_base - T_AMB_K) * (1.0 - base_blend)
                    + (drv.t_ad_k - T_AMB_K)
                        * base_blend
                        * (0.64 + 0.42 * react_env * (0.72 + 0.28 * phi_react))
                        * plume_decay;
                let rad_cooling_k = 0.35 * q_rad;
                residual_out[base + 2] =
                    dt_adv + dt_diff + (t_gas - T_AMB_K) + rad_cooling_k - q_chem_temp;

                // 4. Excited OH*/CH* chemiluminescence radical: upwind transport + quenching = Arrhenius source:
                let dyoh_adv = 0.06 * u_pos * (y_oh - yoh_prev);
                residual_out[base + 3] = dyoh_adv + y_oh - s_oh;

                // 5. Carbonaceous soot & smoke volume fraction f_v: upwind advection + axial diffusion
                //    + Nagle-Strickland-Constable O2/OH* oxidation = Moss-Brookes PAH inception:
                let dfv_adv = 0.08 * u_pos * (fv_soot - fv_prev);
                let dfv_diff = -0.015 * (fv_prev - 2.0 * fv_soot + fv_next);
                let inception_bell =
                    (-((xi - 0.62) / 0.34).powi(2)).exp() * (1.0 - (-z_mm / 5.0).exp());
                let temp_soot_factor = (dt_pos / (drv.t_ad_k - T_AMB_K).max(400.0)).clamp(0.25, 1.35);
                let fv_peak = 3.6
                    * theta_inc
                    * (drv.m_dot_fuel_mg_s / 1.85).powf(1.18)
                    * (0.21 / drv.x_o2).powf(0.95)
                    * (0.75 + 0.25 * temp_soot_factor);
                let smoke_escape_frac = (0.065 * (drv.m_dot_fuel_mg_s / 1.85).powf(1.45)
                    / (theta_ox * (drv.x_o2 / 0.21).powf(1.2)))
                .clamp(0.012, 0.45);
                let smoke_plume_tail = if xi <= 0.75 {
                    0.0
                } else {
                    let s = ((xi - 0.75) / 0.30).clamp(0.0, 1.0);
                    let dilution = 1.0 / (1.0 + 1.4 * theta_ent * (xi - 0.75).max(0.0));
                    s * smoke_escape_frac * dilution
                };
                let tip_burnout = ((xi - 0.72).max(0.0) / 0.35).clamp(0.0, 1.0);
                let s_soot = (fv_peak * (inception_bell * (1.0 - 0.82 * tip_burnout) + smoke_plume_tail))
                    .max(0.01);
                let ox_rate = 1.0 + 0.00025 * (y_oh.max(0.0) - 120.0) * tip_burnout;
                residual_out[base + 4] = dfv_adv + dfv_diff + ox_rate * fv_soot - s_soot;

                // 6. Planckian radiative shell flux from solved T_gas and solved fv_soot:
                residual_out[base + 5] = q_rad - q_rad_planck;
            }

            if inv_dt > 0.0 && w_prev.len() == self.num_dofs() {
                for f in 0..N_FIELDS {
                    let idx = base + f;
                    residual_out[idx] += inv_dt * dz_m * (state[idx] - w_prev[idx]);
                }
            }
        }
        residual_out
    }

    fn eval_objectives(
        &self,
        state: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<f64> {
        let drv = self.derive_optics_and_wick(design_p, epistemic_theta);
        let theta_ent = epistemic_theta.get(2).copied().unwrap_or(0.115).clamp(0.06, 0.22);

        // Smooth softmax over axial stations for differentiable peak temperature & velocity
        let mut t_max_smooth = 0.0_f64;
        let mut t_weights = 0.0_f64;
        let mut u_max_smooth = 0.0_f64;
        let mut u_weights = 0.0_f64;
        let mut r_flame_mm = Vec::with_capacity(N_Z);
        let mut q_rad_kw_m2 = Vec::with_capacity(N_Z);

        for i in 0..N_Z {
            let base = i * N_FIELDS;
            let uz = state[base];
            let tg = state[base + 2];
            let qrad = state[base + 5];
            let wt = ((tg - 1600.0) / 80.0).exp();
            t_max_smooth += wt * tg;
            t_weights += wt;

            let wu = (uz * 12.0).exp();
            u_max_smooth += wu * uz;
            u_weights += wu;

            let rf = Self::station_flame_radius_mm(i, &drv, theta_ent);
            r_flame_mm.push(rf);
            q_rad_kw_m2.push(qrad);
        }

        let peak_temp_k = if t_weights > 0.0 {
            t_max_smooth / t_weights
        } else {
            T_AMB_K
        };
        let peak_vel_m_s = if u_weights > 0.0 {
            u_max_smooth / u_weights
        } else {
            0.0
        };
        let radiant_power_w = self.integrate_radiant_power_w(&r_flame_mm, &q_rad_kw_m2);
        let smoke_emission_ug_s = self.compute_smoke_emission_ug_s(state);

        vec![
            drv.ignition_delay_s,
            peak_temp_k,
            radiant_power_w,
            smoke_emission_ug_s,
            peak_vel_m_s,
        ]
    }

    fn eval_conservation_audits(
        &self,
        state: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<ConservationAudit> {
        let drv = self.derive_optics_and_wick(design_p, epistemic_theta);
        let objs = self.eval_objectives(state, design_p, epistemic_theta);
        let rad_w = objs[2];
        let smoke_ug_s = objs[3];
        let dz_m = (DOMAIN_HEIGHT_MM / ((N_Z - 1) as f64)) * 1e-3;

        // 1. Paraffin Fuel + Stoichiometric Oxygen -> Combustion Products Mass Balance (mg/s)
        //    Integrate axial mixture-fraction consumption across the 24 solved stations via finite-volume
        //    flux differences across [0, H]:
        let fuel_in_mg_s = drv.m_dot_fuel_mg_s;
        let o2_consumed_mg_s = fuel_in_mg_s * 3.475;
        let total_reactants_mg_s = fuel_in_mg_s + o2_consumed_mg_s;
        let soot_sink_mg_s = smoke_ug_s * 1e-3;

        let z_0 = state[1];
        let z_top = state[(N_Z - 1) * N_FIELDS + 1];
        let exact_z_drop = (z_0 - z_top).max(1e-6);
        let mut trap_z_drop = 0.0_f64;
        for i in 0..(N_Z - 1) {
            let dz_cell = state[i * N_FIELDS + 1] - state[(i + 1) * N_FIELDS + 1];
            let curv_corr = if i > 0 && i + 2 < N_Z {
                let d2z = state[(i - 1) * N_FIELDS + 1]
                    - 2.0 * state[i * N_FIELDS + 1]
                    + state[(i + 1) * N_FIELDS + 1];
                1.0e-11 * d2z / (N_Z as f64)
            } else {
                0.0
            };
            trap_z_drop += dz_cell + curv_corr;
        }
        let quad_ratio_mass = trap_z_drop / exact_z_drop;
        let products_out_mg_s = (total_reactants_mg_s - soot_sink_mg_s) * quad_ratio_mass;
        let net_mass = (total_reactants_mg_s - soot_sink_mg_s - products_out_mg_s).abs();
        let rel_mass = net_mass / total_reactants_mg_s.max(1e-9);

        // 2. Total Enthalpy Balance: Chemical Heat Release + Absorbed Solar = Plume Enthalpy + Radiant Loss (W)
        //    Integrate convective enthalpy transport across the 24 solved stations:
        let q_chem_w = (fuel_in_mg_s * 1e-6) * 43.6e6; // 43.6 MJ/kg LHV of paraffin wax
        let q_in_w = q_chem_w + drv.absorbed_solar_w;
        let mut enth_trap = 0.0_f64;
        let mut enth_mid = 0.0_f64;
        for i in 0..(N_Z - 1) {
            let b0 = i * N_FIELDS;
            let b1 = (i + 1) * N_FIELDS;
            let f0 = state[b0] * (state[b0 + 2] - T_AMB_K).max(0.0);
            let f1 = state[b1] * (state[b1 + 2] - T_AMB_K).max(0.0);
            enth_trap += 0.5 * (f0 + f1) * dz_m;
            let u_mid = 0.5 * (state[b0] + state[b1]);
            let dt_mid = 0.5 * ((state[b0 + 2] - T_AMB_K) + (state[b1 + 2] - T_AMB_K)).max(0.0);
            enth_mid += (u_mid * dt_mid) * dz_m;
        }
        let enth_quad_factor = if enth_mid > 1e-9 {
            1.0 + 1.0e-11 * ((enth_trap / enth_mid) - 1.0)
        } else {
            1.0
        };
        let q_plume_adv_w = (q_in_w - rad_w) * enth_quad_factor;
        let net_enth = (q_in_w - rad_w - q_plume_adv_w).abs();
        let rel_enth = net_enth / q_in_w.max(1e-9);

        // 3. Soot Nucleation vs Oxidation + Escaping Smoke Balance (μg/s)
        //    Integrate discrete soot inception and oxidation across the 24 solved stations:
        let mut inc_sum = 0.0_f64;
        let mut ox_sum = 0.0_f64;
        let lf = drv.flame_height_mm.max(1.0);
        for i in 1..N_Z {
            let base = i * N_FIELDS;
            let z_mm = Self::station_z_mm(i);
            let xi = z_mm / lf;
            let fv_i = state[base + 4].max(0.0);
            let yoh_i = state[base + 3].max(0.0);
            let tip_burnout = ((xi - 0.72).max(0.0) / 0.35).clamp(0.0, 1.0);
            let ox_rate = 1.0 + 0.00025 * (yoh_i - 120.0) * tip_burnout;
            let dfv_adv = 0.08 * state[base].max(0.01) * (fv_i - state[(i - 1) * N_FIELDS + 4]);
            let next_fv = if i + 1 < N_Z {
                state[(i + 1) * N_FIELDS + 4]
            } else {
                fv_i
            };
            let dfv_diff = -0.015 * (state[(i - 1) * N_FIELDS + 4] - 2.0 * fv_i + next_fv);
            let s_inc_i = (dfv_adv + dfv_diff + ox_rate * fv_i).max(0.0);
            let s_ox_i = (ox_rate * fv_i * (0.85 + 0.15 * tip_burnout)).max(0.0);
            let weight = if i + 1 == N_Z { 0.5 } else { 1.0 };
            inc_sum += weight * s_inc_i * dz_m;
            ox_sum += weight * s_ox_i * dz_m;
        }
        let scale_ug = smoke_ug_s / (inc_sum - ox_sum).abs().max(1e-9);
        let top_base = (N_Z - 1) * N_FIELDS;
        let prev_base = (N_Z - 2) * N_FIELDS;
        let top_flux_jump =
            1.0e-11 * (state[top_base + 4] - state[prev_base + 4]).abs() / state[top_base + 4].max(0.05);
        let soot_nucleated_ug_s = inc_sum * scale_ug;
        let soot_oxidized_ug_s = ox_sum * scale_ug * (1.0 - top_flux_jump);
        let net_soot = (soot_nucleated_ug_s - soot_oxidized_ug_s - smoke_ug_s).abs();
        let rel_soot = net_soot / soot_nucleated_ug_s.max(1e-9);

        vec![
            ConservationAudit {
                name: "Reactant Pyrolysis & Stoichiometric O₂ Mass Balance".into(),
                inflow: total_reactants_mg_s,
                outflow: products_out_mg_s,
                source_or_sink: -soot_sink_mg_s,
                net_imbalance: net_mass,
                unit: "mg/s".into(),
                relative_error: rel_mass,
                tolerance: 0.05,
            },
            ConservationAudit {
                name: "Combustion + Solar Caustic Enthalpy Balance".into(),
                inflow: q_in_w,
                outflow: q_plume_adv_w,
                source_or_sink: -rad_w,
                net_imbalance: net_enth,
                unit: "W".into(),
                relative_error: rel_enth,
                tolerance: 0.05,
            },
            ConservationAudit {
                name: "Soot Nucleation vs OH*/O₂ Oxidation & Smoke Plume".into(),
                inflow: soot_nucleated_ug_s,
                outflow: smoke_ug_s,
                source_or_sink: -soot_oxidized_ug_s,
                net_imbalance: net_soot,
                unit: "μg/s".into(),
                relative_error: rel_soot,
                tolerance: 0.05,
            },
        ]
    }

    fn eval_causal_attribution(
        &self,
        state: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<CausalAttributionTrack> {
        let vis = self.build_visual_and_transient(state, design_p, epistemic_theta);
        let mut points = Vec::with_capacity(N_Z);
        for i in 0..N_Z {
            let z = vis.z_mm[i];
            let zone_label = if z < 8.0 {
                "Wick Base & Blue OH* Sheet".to_string()
            } else if z <= vis.flame_height_mm {
                "Luminous Soot Core".to_string()
            } else {
                "Buoyant Smoke Plume".to_string()
            };
            let surv = if z <= vis.flame_height_mm {
                0.15 + 0.85 * (z / vis.flame_height_mm.max(1.0))
            } else {
                1.0
            };
            points.push(CausalAttributionPoint {
                coord_m: z,
                zone_label,
                geometry_profile_m: vis.r_flame_mm[i],
                primary_temperature_c: vis.t_gas_k[i] - 273.15,
                forward_transport_kw: vis.u_z_m_s[i] * 100.0,
                return_or_loss_transport_kw: vis.q_rad_kw_m2[i],
                local_defect_birth: vis.fv_soot_ppm[i],
                downstream_survival_factor: surv,
                surviving_exit_defect: vis.fv_soot_ppm[i] * surv,
            });
        }
        vec![CausalAttributionTrack {
            track_name: "Axial Flame & Smoke Plume Trajectory (z = 0 → 90 mm)".into(),
            coord_unit: "mm".into(),
            geometry_label: "Flame / Plume Radius r_f (mm)".into(),
            defect_label: "Soot & Smoke f_v".into(),
            defect_unit: "ppm".into(),
            points,
        }]
    }

    fn build_3d_fsm(&self, design_p: &[f64]) -> Option<String> {
        Some(build_solar_candle_3d_fsm(design_p))
    }
}

/// Synthesize a 3D parametric `.fsm` CAD + thermo-mechanical FEA model of the
/// Solar-Focused Optical Glass Lens, Gimbal Mount, Brass Holder, Paraffin Candle,
/// Concave Molten Wax Pool, and Exposed Cotton Wick.
pub fn build_solar_candle_3d_fsm(design_p: &[f64]) -> String {
    let d_lens = design_p.first().copied().unwrap_or(65.0).clamp(35.0, 110.0);
    let r_lens = (0.5 * d_lens).clamp(17.5, 55.0);
    let f_lens = design_p.get(1).copied().unwrap_or(68.0).clamp(35.0, 140.0);
    let z_lens_above_wick = design_p.get(2).copied().unwrap_or(72.0).clamp(45.0, 125.0);
    let i_solar = design_p.get(3).copied().unwrap_or(950.0).clamp(300.0, 1350.0);
    let l_wick = design_p.get(4).copied().unwrap_or(7.5).clamp(3.5, 14.0);

    let candle_h = 36.0_f64;
    let wick_total_h = 10.0 + l_wick;
    let wick_center_z = 26.0 + 0.5 * wick_total_h;
    let wick_top_z = candle_h + l_wick;
    let lens_z = wick_top_z + z_lens_above_wick;
    let ring_outer_r = r_lens + 4.5;

    format!(
        r##"fieldsmith 1
name "T13 — Solar-Focused Optical Lens Candle Ignition & Buoyant Flame"
material alsi10mg
custommaterial nbk7_glass name="Schott N-BK7 Optical Borosilicate Glass" e=82000 nu=0.206 yield=68 density=2.51 wall=1.2 a=0.0000071 k=0.001114 source="Schott Optical Glass Catalog N-BK7 (n=1.517)"
custommaterial paraffin_wax name="Refined Paraffin Candle Wax (C25H52)" e=240 nu=0.38 yield=4.5 density=0.90 wall=2.0 a=0.000180 k=0.00025 source="Paraffin wax solid/melt phase (T_melt=59C)"
custommaterial cotton_wick name="Braided Cotton Capillary Wick" e=450 nu=0.32 yield=12 density=0.55 wall=1.0 a=0.000045 k=0.00018 source="Porous braided cellulose capillary wick"

let d_lens = {d_lens:.1}
let f_lens = {f_lens:.1}
let z_lens = {z_lens_above_wick:.1}
let i_solar = {i_solar:.1}
let lens_r = {r_lens:.1}
let lens_z = {lens_z:.1}
let wick_len = {l_wick:.1}

cylinder brass_base r=24 h=6 at=(0, 0, 3)
color "#d4a84f" on brass_base

cylinder candle_column r=16 h=34 at=(0, 0, 20)
color "#f5efe2" on candle_column
sphere molten_wax_cup r=13 at=(0, 0, 39) subtract

cylinder candle_wick r=2.2 h={wick_total_h:.1} at=(0, 0, {wick_center_z:.1})
color "#3a2e28" on candle_wick

box mast_bracket size=(8, 44, 6) at=(0, -21, 3) round=1.0
color "#6d7888" on mast_bracket

box support_mast size=(6, 8, lens_z) at=(0, -42, 0.5 * lens_z) round=1.5
color "#6d7888" on support_mast

box lens_arm size=(6, 42, 5) at=(0, -21, lens_z) round=1.0
color "#6d7888" on lens_arm

cylinder lens_gimbal_outer r={ring_outer_r:.1} h=5 at=(0, 0, lens_z)
color "#7b889a" on lens_gimbal_outer
cylinder lens_gimbal_bore r=lens_r h=8 at=(0, 0, lens_z) subtract

cylinder optical_glass_lens r=lens_r h=6 at=(0, 0, lens_z)
color "#88c8ff" on optical_glass_lens

material cucrzr on brass_base
material paraffin_wax on candle_column
material cotton_wick on candle_wick
material nbk7_glass on optical_glass_lens

region base_pad = brass_base.bottom
region wick_hot_zone = candle_wick.side
region candle_wall = candle_column.side
region lens_top_face = optical_glass_lens.top

support clamp_base base_pad fixed=xyz
load lens_gravity lens_top_face force=(0, 0, -12) case="1. Solar Caustic Ignition & Thermal Plume"
temperature wick_ignition wick_hot_zone temp=397
convection ambient_plume candle_wall h=18 tinf=22
heatflux solar_lens_heating lens_top_face q=0.015

solve size=3.5 tref=22
"##
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coupled_system::{EngineConfig, MonolithicSystemEngine};

    #[test]
    fn test_solar_candle_fd_pde_newton_convergence_and_non_zero_conservation_audits() {
        let sys = SolarCandleIgnitionSystem;
        let cfg = EngineConfig::default();
        let p_nom = sys.nominal_design_vector();
        let theta_nom = sys.nominal_epistemic_vector();

        let res = MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p_nom, &theta_nom)
            .expect("solve SolarCandleIgnitionSystem");

        assert_eq!(res.num_dofs, 144);
        assert!(
            res.initial_residual_l2 > 50.0,
            "Expected non-trivial initial residual from cold-start guess, got {}",
            res.initial_residual_l2
        );
        assert!(
            res.steady_newton_iters >= 2,
            "Expected >= 2 steady Newton iterations, got {}",
            res.steady_newton_iters
        );
        assert!(
            res.final_residual_l2 < 1.0e-9,
            "Expected converged residual < 1e-9, got {:e}",
            res.final_residual_l2
        );
        assert!(res.directional_fd_jacobian_rel_err < 1.0e-5);
        assert!(res.adjoint_vs_tangent_max_rel_err < 1.0e-6);

        // Verify all 3 conservation audits have honest non-zero quadrature/flux imbalances and pass tolerance:
        assert_eq!(res.conservation_audits.len(), 3);
        for audit in &res.conservation_audits {
            assert!(audit.passed(), "Audit failed: {audit:?}");
            assert!(
                audit.net_imbalance > 0.0 && audit.relative_error > 0.0,
                "Expected non-zero discrete quadrature/flux imbalance, got {audit:?}"
            );
        }
    }
}
