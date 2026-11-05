//! 4-Inch DeadCat FPV Drone Airframe + Gemini Dual-Band (`900 MHz` + `2.4 GHz` ELRS Xrossband)
//! Antenna Design, VNA Scattering Matrix (`S₁₁`, `S₂₁`, `VSWR`), Smith Chart Impedance Trajectory,
//! 3D Radiation Lobe (`G(θ, φ)`), and Anisotropic Carbon-Fibre (`CFRP`) Coupling Engine.
//!
//! Models the coupled electromagnetic, dielectric, and structural interaction between:
//! 1. **Dual-Band Gemini Diversity Antenna System** (`868 / 915 MHz` Low-Band + `2.40–2.48 GHz`
//!    High-Band on two spatial/polarisation diversity ports `Port 1` & `Port 2`), supporting both:
//!    - **Coaxial T-Dipole / Sleeve V-Tail Pod** (`RearTpuVDipolePod`)
//!    - **Conformal Frame-Integrated Folded FPC/PCB Dipole** (`ConformalArmFoldedFpc`)
//!    - **Split Front-Horizontal / Rear-Vertical Orthogonal Diversity** (`SplitFrontRearOrthogonal`)
//! 2. **4-Inch DeadCat Carbon-Fibre (`CFRP`) Airframe & Mounted Components**:
//!    - Asymmetric DeadCat swept-forward front arms (`105°` camera clearance) and rear arms (`68°`)
//!      with finite anisotropic sheet conductivity `σ_cfrp` (`1.5×10⁴ S/m` in-plane) and Leontovich
//!      surface impedance `Z_s = (1 + j) / (σ_cfrp δ_skin)`.
//!    - `4 ×` `1404` brushless motors (`Ø18 × 14 mm` steel/NdFeB/Cu cans), `20×20 mm` FC/ESC stack
//!      (`FR-4 G10` + copper ground planes), `Al 7075-T6` FPV camera cage, and top-mounted `4S 850 mAh`
//!      LiPo battery pack.
//!    - 3D-printed TPU / PA12 / PC antenna pod dielectric loading (`ε_r`, `tan δ`), which slows the
//!      phase velocity by `1 / √ε_eff` and shifts the unloaded resonance downward unless pre-compensated.
//!
//! Provides:
//! - **Full Dual-Band VNA Frequency Sweep** (`820–980 MHz` and `2300–2580 MHz`) with complex input
//!   impedance `Z_in(f) = R_in(f) + j X_in(f)`, complex reflection coefficient `Γ(f)` for the
//!   **50 Ω Smith Chart**, return loss `S₁₁(dB)`, `VSWR`, 2-port mutual coupling `S₂₁(dB)`,
//!   Envelope Correlation Coefficient (`ECC`), and radiation efficiency `η_rad(%)` (separating
//!   radiation resistance `R_rad` from CFRP eddy-current ohmic loss `R_cfrp` and TPU dielectric loss).
//! - **3D Far-Field Radiation Pattern `G(θ, φ)`** and principal E-plane / H-plane / Yaw-Horizon /
//!   Roll-Bank polar cuts, capturing carbon-arm and motor/battery shadow nulls, `p10` blind-spot
//!   gain floor (`dBi`), and dual-band ExpressLRS Gemini link range (`km` at `100 mW`).
//! - **Automated Multi-Objective Geometry & Matching Optimiser** (coordinate/Nelder-Mead search +
//!   analytical L-network impedance matching) that tunes CFRP standoff `d_cfrp`, diversity skew
//!   angle `α_skew`, `900 MHz` & `2.4 GHz` element lengths (`L_900`, `L_2400`), and matching
//!   shunt/series `L`/`C` to center both bands on the `50 Ω` Smith Chart while maximizing `η_rad`
//!   and eliminating deep blind spots.
//! - **Canonical 3D `.fsm` CSG Generator** (`build_deadcat_drone_fsm`) producing the complete
//!   4-inch DeadCat drone assembly with parametric `let` variables and `M2` Ti-6Al-4V hardware.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use crate::coupled_system::{
    CausalAttributionPoint, CausalAttributionTrack, ConservationAudit, CoupledEngineeringSystem,
    DesignParameter, DesignParameterCategory, EpistemicParameter, FieldDescriptor,
    ObjectiveDescriptor, ObjectiveKind,
};

pub const C0_MM_PER_GHZ: f64 = 299.792_458; // c = 299.792458 mm · GHz
pub const ETA0_OHM: f64 = 376.730_313_668; // Free-space wave impedance η₀ (Ω)
pub const MU0_H_PER_M: f64 = 4.0e-7 * PI;
pub const Z0_OHM: f64 = 50.0; // Standard 50 Ω coax / VNA port impedance

/// Supported Gemini dual-band (`900 MHz` + `2.4 GHz`) antenna integration topologies on a 4" DeadCat frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DroneAntennaTopology {
    /// Rear 3D-printed TPU V-tail pod holding two orthogonal coaxial sleeve/trap T-dipoles (`Port 1` & `Port 2`).
    RearTpuVDipolePod,
    /// Conformal folded FPC/PCB dual-band dipoles embedded along the dielectric arm/canopy guards.
    ConformalArmFoldedFpc,
    /// Split diversity: Port 1 horizontal T-dipole on rear TPU standoff, Port 2 vertical dipole on front camera canopy.
    SplitFrontRearOrthogonal,
}

impl DroneAntennaTopology {
    pub fn from_str_id(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "conformal_arm_folded_fpc" | "conformal" | "fpc" => Self::ConformalArmFoldedFpc,
            "split_front_rear_orthogonal" | "split" | "front_rear" => {
                Self::SplitFrontRearOrthogonal
            }
            _ => Self::RearTpuVDipolePod,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RearTpuVDipolePod => "rear_tpu_v_dipole_pod",
            Self::ConformalArmFoldedFpc => "conformal_arm_folded_fpc",
            Self::SplitFrontRearOrthogonal => "split_front_rear_orthogonal",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::RearTpuVDipolePod => {
                "Rear TPU V-Tail Pod (Dual Coaxial Gemini T-Dipoles, 900 MHz + 2.4 GHz)"
            }
            Self::ConformalArmFoldedFpc => {
                "Conformal Frame-Integrated FPC Folded Dipoles (Arm / Canopy Dielectric Guards)"
            }
            Self::SplitFrontRearOrthogonal => {
                "Split Front/Rear Orthogonal Diversity (Rear Horizontal + Front Canopy Vertical)"
            }
        }
    }
}

/// Input specification for a 4" DeadCat FPV drone + Gemini dual-band antenna VNA & 3D lobe analysis.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DroneAntennaDesignSpec {
    pub topology: DroneAntennaTopology,
    /// Center target frequency for Low-Band (`0.868` GHz EU868 or `0.915` GHz FCC915).
    pub target_low_freq_ghz: f64,
    /// Center target frequency for High-Band (`2.440` GHz ISM / ELRS 2.4).
    pub target_high_freq_ghz: f64,
    /// Standoff distance `d_cfrp` from the nearest CFRP carbon plate/arm edge to the antenna feed/arms (mm).
    pub cfrp_standoff_mm: f64,
    /// Total tip-to-tip active length of the 900 MHz low-band dipole element `L_900` (mm).
    pub low_band_arm_length_mm: f64,
    /// Total tip-to-tip active length of the 2.4 GHz high-band sleeve/inner dipole element `L_2400` (mm).
    pub high_band_arm_length_mm: f64,
    /// Included angle `α_skew` between Gemini Port 1 and Port 2 diversity dipole axes (degrees, ideal ~90°).
    pub diversity_skew_deg: f64,
    /// Elevation offset `z_pod` of the antenna feed above the CFRP top plate plane (mm).
    pub elevation_above_plate_mm: f64,
    /// Effective wall thickness `t_tpu` of the 3D-printed TPU/PA12 dielectric antenna enclosure around the elements (mm).
    pub dielectric_pod_wall_mm: f64,
    /// Relative permittivity `ε_r` of the 3D-printed antenna mount (`2.8` TPU 95A, `3.1` PETG, `2.5` PA12, `2.2` PEEK).
    pub pod_eps_r: f64,
    /// Dielectric loss tangent `tan δ` of the 3D-printed antenna mount (`0.018` TPU, `0.008` PA12, `0.003` PEEK).
    pub pod_loss_tan: f64,
    /// Carbon-fibre frame effective in-plane conductivity `σ_cfrp` (S/m, typically `1.5e4` for 3K woven CFRP).
    pub cfrp_conductivity_s_per_m: f64,
    /// DeadCat frame wheelbase diagonal (motor-to-motor, mm; 4-inch class = `172.0` mm).
    pub wheelbase_mm: f64,
    /// DeadCat front arm forward sweep angle from lateral X axis (degrees; DeadCat clears props at `18°`).
    pub front_arm_sweep_deg: f64,
    /// DeadCat rear arm backward sweep angle from lateral X axis (degrees; typically `34°`).
    pub rear_arm_sweep_deg: f64,
    /// Series matching reactance `X_ser` at the feed (Ω at `915 MHz`, scaled by frequency; `0.0` = direct 50 Ω coax feed).
    pub match_series_nh: f64,
    /// Shunt matching capacitance `C_shunt` across the feed (pF; `0.0` = direct 50 Ω coax feed).
    pub match_shunt_pf: f64,
    /// Transmitter RF power per Gemini chain (mW; e.g. `100.0` mW = `20 dBm` per port).
    pub tx_power_mw: f64,
}

impl Default for DroneAntennaDesignSpec {
    fn default() -> Self {
        Self {
            topology: DroneAntennaTopology::RearTpuVDipolePod,
            target_low_freq_ghz: 0.915,
            target_high_freq_ghz: 2.440,
            cfrp_standoff_mm: 24.0,
            low_band_arm_length_mm: 136.5,
            high_band_arm_length_mm: 51.2,
            diversity_skew_deg: 86.0,
            elevation_above_plate_mm: 16.0,
            dielectric_pod_wall_mm: 1.8,
            pod_eps_r: 2.75,
            pod_loss_tan: 0.014,
            cfrp_conductivity_s_per_m: 15_000.0,
            wheelbase_mm: 172.0,
            front_arm_sweep_deg: 18.0,
            rear_arm_sweep_deg: 34.0,
            match_series_nh: 1.8,
            match_shunt_pf: 0.65,
            tx_power_mw: 100.0,
        }
    }
}

impl DroneAntennaDesignSpec {
    /// Unoptimised "compact tucked" factory build where the builder mounts the Gemini antenna
    /// flush against the rear CFRP standoffs (`d_cfrp = 6.5 mm`), uses free-space half-wave lengths
    /// (`156 mm` / `59 mm`) inside a thick TPU boot (`t = 3.2 mm`), and angles the diversity arms at `38°`.
    pub fn unoptimised_tucked_preset() -> Self {
        Self {
            topology: DroneAntennaTopology::RearTpuVDipolePod,
            target_low_freq_ghz: 0.915,
            target_high_freq_ghz: 2.440,
            cfrp_standoff_mm: 6.5,
            low_band_arm_length_mm: 156.0,
            high_band_arm_length_mm: 59.0,
            diversity_skew_deg: 38.0,
            elevation_above_plate_mm: 4.0,
            dielectric_pod_wall_mm: 3.2,
            pod_eps_r: 2.95,
            pod_loss_tan: 0.022,
            cfrp_conductivity_s_per_m: 15_000.0,
            wheelbase_mm: 172.0,
            front_arm_sweep_deg: 18.0,
            rear_arm_sweep_deg: 34.0,
            match_series_nh: 0.0,
            match_shunt_pf: 0.0,
            tx_power_mw: 100.0,
        }
    }
}

/// One frequency point in the VNA sweep (`Z_in`, Smith Chart `Γ`, `S₁₁`, `S₂₁`, `VSWR`, `η_rad`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VnaSamplePoint {
    pub freq_ghz: f64,
    pub band: String, // "low_900" or "high_2400"
    /// Input resistance `R_in` (Ω).
    pub z_real_ohm: f64,
    /// Input reactance `X_in` (Ω).
    pub z_imag_ohm: f64,
    /// Normalized Smith Chart resistance `r = R_in / 50`.
    pub smith_r: f64,
    /// Normalized Smith Chart reactance `x = X_in / 50`.
    pub smith_x: f64,
    /// Complex reflection coefficient real part `Re(Γ)`.
    pub gamma_re: f64,
    /// Complex reflection coefficient imaginary part `Im(Γ)`.
    pub gamma_im: f64,
    /// Reflection magnitude `|Γ| ∈ [0, 1]`.
    pub gamma_mag: f64,
    /// Return loss `S₁₁ = 20 log₁₀ |Γ|` (dB, negative; `< -10 dB` is matched).
    pub s11_db: f64,
    /// Voltage Standing Wave Ratio `VSWR = (1 + |Γ|) / (1 - |Γ|)`.
    pub vswr: f64,
    /// Mutual coupling `S₂₁` between Gemini Port 1 and Port 2 diversity chains (dB, negative; `< -15 dB` is well isolated).
    pub s21_db: f64,
    /// Radiation efficiency `η_rad = R_rad / (R_rad + R_cfrp_eddy + R_dielectric + R_metal)` in `[0, 1]`.
    pub radiation_efficiency: f64,
    /// Total efficiency `η_tot = η_rad · (1 - |Γ|²)` including mismatch reflection loss.
    pub total_efficiency: f64,
    /// Fraction of accepted power dissipated as eddy-current Joule heat in the CFRP carbon frame.
    pub cfrp_absorption_fraction: f64,
}

/// Polar cut point (`angle_deg ∈ [0, 360)`, `gain_dbi` for Port 1, Port 2, and Gemini Diversity combined).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolarLobePoint {
    pub angle_deg: f64,
    pub port1_gain_dbi: f64,
    pub port2_gain_dbi: f64,
    pub gemini_combined_gain_dbi: f64,
    pub cfrp_shadow_loss_db: f64,
}

/// 3D Spherical Lobe Vertex (`theta_deg`, `phi_deg`, `gain_dbi`, `[x, y, z]` in drone mm space for viewport overlay).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lobe3dVertex {
    pub theta_deg: f64,
    pub phi_deg: f64,
    pub gain_dbi: f64,
    pub radius_mm: f64,
    pub pos_mm: [f64; 3],
}

/// Summary metrics for one frequency band (`900 MHz` or `2.4 GHz`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BandPerformanceSummary {
    pub band_label: String,
    pub target_freq_ghz: f64,
    pub resonant_freq_ghz: f64,
    pub freq_detuning_mhz: f64,
    pub z_at_target_ohm: [f64; 2],
    pub gamma_at_target: [f64; 2],
    pub s11_at_target_db: f64,
    pub vswr_at_target: f64,
    pub s21_isolation_db: f64,
    pub envelope_correlation_ecc: f64,
    pub bandwidth_s11_below_10db_mhz: f64,
    pub radiation_efficiency_pct: f64,
    pub total_efficiency_pct: f64,
    pub cfrp_power_loss_pct: f64,
    pub peak_gain_dbi: f64,
    pub p10_blind_spot_floor_dbi: f64,
    pub min_null_gain_dbi: f64,
    pub front_to_back_db: f64,
    pub estimated_los_range_km: f64,
    pub yaw_horizon_cut: Vec<PolarLobePoint>,
    pub pitch_elevation_cut: Vec<PolarLobePoint>,
    pub roll_bank_cut: Vec<PolarLobePoint>,
    pub lobe_3d_rings: Vec<Vec<Lobe3dVertex>>,
}

/// Complete VNA, Smith Chart, and 3D Radiation Lobe solution for the 4" DeadCat + Gemini Dual-Band Drone.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DroneAntennaAnalysisReport {
    pub spec: DroneAntennaDesignSpec,
    pub low_band_sweep: Vec<VnaSamplePoint>,
    pub high_band_sweep: Vec<VnaSamplePoint>,
    pub low_band_summary: BandPerformanceSummary,
    pub high_band_summary: BandPerformanceSummary,
    /// Recommended L-network matching suggestion (`series_nh`, `shunt_pf`) for the current geometry.
    pub suggested_match_series_nh: f64,
    pub suggested_match_shunt_pf: f64,
    /// Multi-objective composite score (lower is better; `< 0` means all VNA & lobe targets pass).
    pub composite_cost_j: f64,
    pub all_targets_met: bool,
    pub engineering_notes: Vec<String>,
}

/// Result of running the automated Gemini Dual-Band Antenna Geometry & Matching Optimiser.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DroneAntennaOptimisationReport {
    pub initial_report: DroneAntennaAnalysisReport,
    pub optimised_report: DroneAntennaAnalysisReport,
    pub iterations: usize,
    pub fsm_3d: String,
}

/// Compute the complex reflection coefficient `Γ = (Z - Z₀) / (Z + Z₀)` for `Z = R + jX` and `Z₀ = 50 Ω`.
#[inline]
pub fn complex_gamma_50(r_ohm: f64, x_ohm: f64) -> (f64, f64, f64) {
    let r_safe = r_ohm.max(0.05);
    let num_re = r_safe - Z0_OHM;
    let num_im = x_ohm;
    let den_re = r_safe + Z0_OHM;
    let den_im = x_ohm;
    let den_norm = (den_re * den_re + den_im * den_im).max(1e-12);
    let g_re = (num_re * den_re + num_im * den_im) / den_norm;
    let g_im = (num_im * den_re - num_re * den_im) / den_norm;
    let g_mag = (g_re * g_re + g_im * g_im).sqrt().clamp(1e-6, 0.999_99);
    (g_re, g_im, g_mag)
}

/// Apply an L-network impedance match (`L_series` in nH, `C_shunt` in pF across load) at `freq_ghz`.
#[inline]
pub fn apply_l_network_match(
    r_raw: f64,
    x_raw: f64,
    freq_ghz: f64,
    series_nh: f64,
    shunt_pf: f64,
) -> (f64, f64) {
    let omega = 2.0 * PI * freq_ghz * 1.0e9;
    // 1. Shunt capacitor C_shunt (pF = 1e-12 F) in parallel with Z_raw = R_raw + j X_raw
    let (r_after_shunt, x_after_shunt) = if shunt_pf.abs() > 1e-6 {
        let b_c = omega * (shunt_pf * 1.0e-12); // susceptance B_c (S)
        let z_norm2 = (r_raw * r_raw + x_raw * x_raw).max(1e-9);
        let g_raw = r_raw / z_norm2;
        let b_raw = -x_raw / z_norm2;
        let b_tot = b_raw + b_c;
        let y_norm2 = (g_raw * g_raw + b_tot * b_tot).max(1e-12);
        (g_raw / y_norm2, -b_tot / y_norm2)
    } else {
        (r_raw, x_raw)
    };
    // 2. Series inductor L_series (nH = 1e-9 H)
    let x_l = omega * (series_nh * 1.0e-9);
    (r_after_shunt.max(0.2), x_after_shunt + x_l)
}

/// Core physics evaluator for a single frequency point `freq_ghz` in either the `900 MHz` or `2.4 GHz` band.
///
/// Uses the induced-EMF thin-wire dipole formulation coupled to:
/// - **Dielectric enclosure wave-slowing** (`ε_eff` from `t_tpu` and `ε_r`)
/// - **Anisotropic CFRP lossy ground-plane image & Leontovich eddy-current mutual impedance**
///   `Z_12(k d_eff) · R_cfrp_refl`
/// - **Motor-bell & LiPo parasitic capacitive/inductive loading**
pub fn evaluate_vna_point(
    spec: &DroneAntennaDesignSpec,
    freq_ghz: f64,
    is_high_band: bool,
) -> VnaSamplePoint {
    let lambda_0_mm = C0_MM_PER_GHZ / freq_ghz.max(0.1);
    let k0 = 2.0 * PI / lambda_0_mm; // rad/mm

    // 1. Effective permittivity ε_eff of the 3D-printed TPU/PA12 antenna pod + CFRP capacitive proximity
    let fill_factor = (spec.dielectric_pod_wall_mm / (spec.dielectric_pod_wall_mm + 2.4)).clamp(0.0, 0.85);
    let cfrp_eff_dist_mm = (spec.cfrp_standoff_mm.powi(2) + (0.75 * spec.elevation_above_plate_mm).powi(2))
        .sqrt()
        .max(2.0);
    // Proximity to the carbon plate increases effective fringe capacitance
    let proximity_cap_loading = 0.22 * (-cfrp_eff_dist_mm / 14.0).exp();
    let eps_eff = 1.0 + fill_factor * (spec.pod_eps_r - 1.0) * 0.42 + proximity_cap_loading;
    let v_factor = 1.0 / eps_eff.sqrt();

    // 2. Active dipole length L and wire radius a for the active band
    let (active_len_mm, wire_r_mm, band_name) = if is_high_band {
        (spec.high_band_arm_length_mm, 0.65, "high_2400")
    } else {
        (spec.low_band_arm_length_mm, 0.85, "low_900")
    };

    // Topology factor: folded FPC has slightly wider impedance bandwidth and higher surface wave coupling
    let (topo_r_scale, topo_q_scale, topo_cfrp_exposure) = match spec.topology {
        DroneAntennaTopology::RearTpuVDipolePod => (1.0, 1.0, 1.0),
        DroneAntennaTopology::ConformalArmFoldedFpc => (1.12, 0.82, 1.35),
        DroneAntennaTopology::SplitFrontRearOrthogonal => (1.03, 0.94, 0.88),
    };

    // Electrical half-wave resonance frequency of the dielectric-coated element
    // In free space, a thin dipole of diameter 2a resonates at L ≈ 0.475 λ_0.
    let end_shortening = 0.476;
    let f_res_unloaded_ghz = (C0_MM_PER_GHZ * end_shortening * v_factor) / active_len_mm.max(10.0);

    // 3. CFRP Leontovich surface impedance Z_s = √(ω μ₀ / (2 σ_cfrp)) · (1 + j)
    let omega = 2.0 * PI * freq_ghz * 1.0e9;
    let sigma_cfrp = spec.cfrp_conductivity_s_per_m.max(100.0);
    let skin_depth_m = (2.0 / (omega * MU0_H_PER_M * sigma_cfrp)).sqrt();
    let r_sheet_ohm = 1.0 / (sigma_cfrp * skin_depth_m); // Surface resistance R_s (Ω/sq)

    // Mutual coupling to the CFRP carbon plate image at distance 2 * d_eff
    let kd_image = (2.0 * k0 * cfrp_eff_dist_mm).max(0.15);
    // Spherical Bessel j0(x) = sin(x)/x, y0(x) = -cos(x)/x
    let j0 = kd_image.sin() / kd_image;
    let y0 = -kd_image.cos() / kd_image;

    // When the dipole sits parallel and close to the conductive carbon edge (kd_image << π),
    // the out-of-phase image current cancels radiation resistance R_rad and injects eddy-current
    // resistance R_cfrp_eddy plus inductive/capacitive image reactance X_image.
    let image_coupling = (1.15 / (1.0 + 0.35 * kd_image)).clamp(0.02, 0.92) * topo_cfrp_exposure;
    let r_rad_freespace = 73.1 * topo_r_scale * (freq_ghz / f_res_unloaded_ghz).powf(1.85).clamp(0.25, 2.6);

    // Image radiation cancellation when d_cfrp < λ/8
    let r_rad_effective = (r_rad_freespace * (1.0 - 0.78 * image_coupling * j0.max(0.0))).max(4.5);

    // Eddy current ohmic loss induced in the woven CFRP frame plates & arms
    let r_cfrp_eddy = (r_sheet_ohm * 3.8 + 18.5)
        * (-cfrp_eff_dist_mm / (0.11 * lambda_0_mm + 8.0)).exp()
        * topo_cfrp_exposure;

    // Dielectric loss in the 3D-printed TPU/PA12 pod + copper coax skin loss
    let r_dielectric = 73.1 * fill_factor * spec.pod_loss_tan * (spec.pod_eps_r - 1.0) * 1.65;
    let r_conductor = 0.85 * (freq_ghz).sqrt();

    let r_total_unmatched = r_rad_effective + r_cfrp_eddy + r_dielectric + r_conductor;

    // 4. Input reactance X_in(f): characteristic impedance Z_char of dipole wire + image reactance + dual-band sleeve trap coupling
    let ln_slenderness = ((active_len_mm / wire_r_mm).ln() - 1.0).max(2.2);
    let z_char = 120.0 * ln_slenderness * topo_q_scale / eps_eff.sqrt();
    let frac_detune = (freq_ghz - f_res_unloaded_ghz) / f_res_unloaded_ghz;
    // Induced-EMF reactance slope near half-wave resonance + CFRP image reactive shift
    let x_dipole = z_char * (0.5 * PI * frac_detune).tan().clamp(-4.5, 4.5);
    let x_image = -38.0 * image_coupling * y0;
    // Dual-band sleeve trap interaction between the 900 MHz outer arm and 2.4 GHz inner sleeve
    let trap_ratio = spec.low_band_arm_length_mm / (2.67 * spec.high_band_arm_length_mm.max(15.0));
    let x_trap_crosstalk = if is_high_band {
        14.0 * (trap_ratio - 1.0)
    } else {
        -9.5 * (trap_ratio - 1.0)
    };

    let x_total_unmatched = x_dipole + x_image + x_trap_crosstalk;

    // 5. Apply feed matching L-network (series L in nH, shunt C in pF)
    let (z_real_ohm, z_imag_ohm) = apply_l_network_match(
        r_total_unmatched,
        x_total_unmatched,
        freq_ghz,
        spec.match_series_nh,
        spec.match_shunt_pf,
    );

    let (gamma_re, gamma_im, gamma_mag) = complex_gamma_50(z_real_ohm, z_imag_ohm);
    let s11_db = 20.0 * gamma_mag.log10();
    let vswr = ((1.0 + gamma_mag) / (1.0 - gamma_mag).max(1e-4)).min(50.0);

    // 6. 2-Port Gemini Diversity Mutual Coupling S21(dB)
    // Polarisation decoupling is proportional to cos(α_skew), plus spatial separation and CFRP ground-bounce coupling
    let skew_rad = spec.diversity_skew_deg.to_radians();
    let pol_coupling = skew_rad.cos().abs(); // 0 at 90°, 1 at 0°
    let spatial_sep_mm = match spec.topology {
        DroneAntennaTopology::SplitFrontRearOrthogonal => spec.wheelbase_mm * 0.72,
        DroneAntennaTopology::ConformalArmFoldedFpc => spec.wheelbase_mm * 0.48,
        DroneAntennaTopology::RearTpuVDipolePod => 18.0 + 0.55 * cfrp_eff_dist_mm,
    };
    let k_sep = (k0 * spatial_sep_mm).max(0.35);
    let cfrp_common_mode = 0.14 * (-cfrp_eff_dist_mm / 12.0).exp();
    let s21_lin = ((0.52 * pol_coupling + 0.08) / k_sep.powf(0.85) + cfrp_common_mode).clamp(1e-3, 0.85);
    let s21_db = 20.0 * s21_lin.log10();

    let radiation_efficiency = (r_rad_effective / r_total_unmatched.max(1e-6)).clamp(0.05, 0.99);
    let mismatch_factor = (1.0 - gamma_mag * gamma_mag).clamp(0.01, 1.0);
    let total_efficiency = radiation_efficiency * mismatch_factor;
    let cfrp_absorption_fraction = (r_cfrp_eddy / r_total_unmatched.max(1e-6)).clamp(0.0, 0.95);

    VnaSamplePoint {
        freq_ghz,
        band: band_name.to_string(),
        z_real_ohm,
        z_imag_ohm,
        smith_r: z_real_ohm / Z0_OHM,
        smith_x: z_imag_ohm / Z0_OHM,
        gamma_re,
        gamma_im,
        gamma_mag,
        s11_db,
        vswr,
        s21_db,
        radiation_efficiency,
        total_efficiency,
        cfrp_absorption_fraction,
    }
}

/// Evaluate directional gain `G(θ, φ)` (dBi) for Port 1, Port 2, and Gemini Diversity combined,
/// accounting for:
/// - Dipole orientation and diversity skew angle `α_skew`
/// - DeadCat CFRP frame & LiPo battery forward diffraction / shadowing (`φ ≈ 90°` nose-on when rear-mounted)
/// - `4 ×` brushless motor bell (`1404` steel/NdFeB cans) azimuthal scatterers at the DeadCat arm tips
pub fn evaluate_directional_gain_dbi(
    spec: &DroneAntennaDesignSpec,
    freq_ghz: f64,
    total_eff: f64,
    theta_rad: f64, // Elevation polar angle from +Z zenith (π/2 = horizon plane)
    phi_rad: f64,   // Azimuth angle in XY plane (+Y = π/2 nose forward, -Y = -π/2 tail rear)
) -> (f64, f64, f64, f64) {
    let lambda_mm = C0_MM_PER_GHZ / freq_ghz.max(0.1);
    let k0 = 2.0 * PI / lambda_mm;

    // Unit look vector u = (sin θ cos φ, sin θ sin φ, cos θ)
    let st = theta_rad.sin();
    let ct = theta_rad.cos();
    let ux = st * phi_rad.cos();
    let uy = st * phi_rad.sin();
    let uz = ct;

    // Dipole 1 and Dipole 2 unit axes in the V-tail or split frame
    let half_skew = 0.5 * spec.diversity_skew_deg.to_radians();
    let (a1, a2) = match spec.topology {
        DroneAntennaTopology::RearTpuVDipolePod => {
            // V-tail tilted back 18° in YZ plane, arms splayed ±half_skew in XZ plane
            let tilt = 18.0_f64.to_radians();
            let d1 = [
                half_skew.sin(),
                -half_skew.cos() * tilt.sin(),
                half_skew.cos() * tilt.cos(),
            ];
            let d2 = [
                -half_skew.sin(),
                -half_skew.cos() * tilt.sin(),
                half_skew.cos() * tilt.cos(),
            ];
            (d1, d2)
        }
        DroneAntennaTopology::ConformalArmFoldedFpc => {
            // Mounted along the two rear DeadCat arms (±rear_arm_sweep_deg) with slight Z tilt
            let r_sweep = spec.rear_arm_sweep_deg.to_radians();
            let d1 = [r_sweep.cos(), -r_sweep.sin(), 0.25];
            let d2 = [-r_sweep.cos(), -r_sweep.sin(), 0.25];
            (d1, d2)
        }
        DroneAntennaTopology::SplitFrontRearOrthogonal => {
            // Port 1 horizontal rear X-axis, Port 2 vertical front Z-axis tilted by (90 - skew)
            let d1 = [1.0, 0.0, 0.0];
            let off = (90.0 - spec.diversity_skew_deg).to_radians();
            let d2 = [off.sin(), 0.15, off.cos()];
            (d1, d2)
        }
    };

    let norm3 = |v: [f64; 3]| -> [f64; 3] {
        let m = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
        [v[0] / m, v[1] / m, v[2] / m]
    };
    let a1 = norm3(a1);
    let a2 = norm3(a2);

    let cos_psi1 = (ux * a1[0] + uy * a1[1] + uz * a1[2]).clamp(-1.0, 1.0);
    let cos_psi2 = (ux * a2[0] + uy * a2[1] + uz * a2[2]).clamp(-1.0, 1.0);
    let sin_psi1_sq = (1.0 - cos_psi1 * cos_psi1).max(0.025); // Finite cross-pol floor (-16 dB)
    let sin_psi2_sq = (1.0 - cos_psi2 * cos_psi2).max(0.025);

    // Base half-wave dipole directivity D0 = 1.64 (2.15 dBi)
    let d0_lin = 1.64;

    // CFRP Carbon Frame + LiPo Battery Shadowing & Edge Diffraction:
    // Rear-mounted antennas look through the carbon body + 4S LiPo + Aluminum camera cage when looking forward (+Y, φ = +π/2)
    // and slightly downward (θ > π/2). Rising elevation `elevation_above_plate_mm` and `cfrp_standoff_mm` clear the Fresnel zone!
    let cfrp_eff_dist = (spec.cfrp_standoff_mm.powi(2) + spec.elevation_above_plate_mm.powi(2)).sqrt();
    let forward_align = (uy.max(0.0)).powf(2.2) * (1.0 - (uz - 0.18).powi(2) * 1.8).clamp(0.1, 1.0);
    let clearance_ratio = (cfrp_eff_dist / (0.28 * lambda_mm + 6.0)).clamp(0.08, 2.5);
    let body_shadow_db = (11.5 * forward_align / (0.45 + 1.45 * clearance_ratio.powf(1.3))).clamp(0.0, 16.5);

    // 4 × 1404 Brushless Motor Bell Azimuthal Diffraction Lobes:
    // DeadCat motor azimuths: Front-Right (+18°), Front-Left (162°), Rear-Right (-34°), Rear-Left (-146°)
    let f_rad = spec.front_arm_sweep_deg.to_radians();
    let r_rad = spec.rear_arm_sweep_deg.to_radians();
    let motor_phis = [f_rad, PI - f_rad, -r_rad, -PI + r_rad];
    let mut motor_scatter_db = 0.0;
    for &m_phi in &motor_phis {
        let dphi = (phi_rad - m_phi).cos();
        if dphi > 0.82 {
            let lobe = ((dphi - 0.82) / 0.18).powi(2);
            motor_scatter_db += 2.6 * lobe * st.powi(2) / (0.6 + 0.03 * spec.cfrp_standoff_mm);
        }
    }

    // Reflective constructive/destructive interference from the rear CFRP edge at (-Y)
    let phase_refl = (2.0 * k0 * spec.cfrp_standoff_mm * (-uy).max(0.0)).cos();
    let edge_ripple_db = 1.65 * phase_refl * (-spec.cfrp_standoff_mm / (0.55 * lambda_mm)).exp();

    let cfrp_shadow_loss_db = body_shadow_db + motor_scatter_db;
    let env_gain_offset_db = edge_ripple_db - cfrp_shadow_loss_db;

    let eff_db = 10.0 * total_eff.max(0.01).log10();
    let g1_dbi = 10.0 * (d0_lin * sin_psi1_sq).log10() + eff_db + env_gain_offset_db;

    // For SplitFrontRearOrthogonal, Port 2 is mounted on the front canopy, so its shadow is towards the rear (-Y)!
    let p2_shadow_db = if spec.topology == DroneAntennaTopology::SplitFrontRearOrthogonal {
        let rear_align = ((-uy).max(0.0)).powf(2.2) * st.powi(2);
        (7.5 * rear_align / (0.5 + clearance_ratio)).clamp(0.0, 12.0) + 0.5 * motor_scatter_db
    } else {
        cfrp_shadow_loss_db
    };
    let g2_dbi = 10.0 * (d0_lin * sin_psi2_sq).log10() + eff_db + edge_ripple_db - p2_shadow_db;

    // ExpressLRS Gemini Dual-Chain Diversity Effective Gain (selection + packet-level MRC boost)
    let g1_lin = 10.0_f64.powf(g1_dbi / 10.0);
    let g2_lin = 10.0_f64.powf(g2_dbi / 10.0);
    // Correlation penalty if diversity_skew_deg is far from 90°
    let skew_orthogonality = spec.diversity_skew_deg.to_radians().sin().abs();
    let diversity_combine_lin = g1_lin.max(g2_lin) + 0.45 * skew_orthogonality * g1_lin.min(g2_lin);
    let gemini_dbi = 10.0 * diversity_combine_lin.max(1e-4).log10();

    (g1_dbi, g2_dbi, gemini_dbi, cfrp_shadow_loss_db)
}

/// Summarise a full frequency band (`900 MHz` or `2.4 GHz`) including resonant point, bandwidth,
/// ECC, 2D polar cuts, and 3D spherical lobe mesh rings.
fn summarise_band(
    spec: &DroneAntennaDesignSpec,
    sweep: &[VnaSamplePoint],
    target_freq_ghz: f64,
    band_label: &str,
) -> BandPerformanceSummary {
    // 1. Find point closest to target_freq_ghz and point of minimum |Γ| (resonance)
    let target_pt = sweep
        .iter()
        .min_by(|a, b| {
            (a.freq_ghz - target_freq_ghz)
                .abs()
                .partial_cmp(&(b.freq_ghz - target_freq_ghz).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned()
        .unwrap_or_else(|| evaluate_vna_point(spec, target_freq_ghz, target_freq_ghz > 1.5));

    let res_pt = sweep
        .iter()
        .min_by(|a, b| {
            a.gamma_mag
                .partial_cmp(&b.gamma_mag)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned()
        .unwrap_or_else(|| target_pt.clone());

    // 2. Compute S11 < -10 dB bandwidth in MHz
    let step_mhz = if sweep.len() > 1 {
        ((sweep[sweep.len() - 1].freq_ghz - sweep[0].freq_ghz).abs() * 1000.0)
            / ((sweep.len() - 1) as f64)
    } else {
        5.0
    };
    let matched_count = sweep.iter().filter(|pt| pt.s11_db <= -10.0).count();
    let bandwidth_mhz = (matched_count as f64) * step_mhz;

    // 3. Envelope Correlation Coefficient (ECC) from S-parameters and skew orthogonality
    let s11_sq = target_pt.gamma_mag.powi(2);
    let s21_lin = 10.0_f64.powf(target_pt.s21_db / 20.0);
    let skew_cos = spec.diversity_skew_deg.to_radians().cos().abs();
    let ecc = ((2.0 * target_pt.gamma_mag * s21_lin * skew_cos).powi(2)
        / ((1.0 - s11_sq - s21_lin.powi(2)).max(0.08).powi(2))
        + 0.04 * skew_cos.powi(2))
    .clamp(0.001, 0.95);

    // 4. Generate 3 principal 2D polar cuts (72 steps = 5° resolution)
    let n_ang = 72usize;
    let mut yaw_horizon_cut = Vec::with_capacity(n_ang);
    let mut pitch_elevation_cut = Vec::with_capacity(n_ang);
    let mut roll_bank_cut = Vec::with_capacity(n_ang);

    for i in 0..n_ang {
        let ang_deg = (i as f64) * 360.0 / (n_ang as f64);
        let ang_rad = ang_deg.to_radians();

        // Yaw Horizon Cut (θ = 90°, φ = ang_rad)
        let (p1, p2, gem, sh) = evaluate_directional_gain_dbi(
            spec,
            target_freq_ghz,
            target_pt.total_efficiency,
            0.5 * PI,
            ang_rad,
        );
        yaw_horizon_cut.push(PolarLobePoint {
            angle_deg: ang_deg,
            port1_gain_dbi: p1,
            port2_gain_dbi: p2,
            gemini_combined_gain_dbi: gem,
            cfrp_shadow_loss_db: sh,
        });

        // Pitch Elevation Cut (YZ plane: φ = +90° forward / -90° aft)
        let theta_pitch = ang_rad;
        let (p1_p, p2_p, gem_p, sh_p) = evaluate_directional_gain_dbi(
            spec,
            target_freq_ghz,
            target_pt.total_efficiency,
            theta_pitch,
            0.5 * PI,
        );
        pitch_elevation_cut.push(PolarLobePoint {
            angle_deg: ang_deg,
            port1_gain_dbi: p1_p,
            port2_gain_dbi: p2_p,
            gemini_combined_gain_dbi: gem_p,
            cfrp_shadow_loss_db: sh_p,
        });

        // Roll Bank Cut (XZ plane: φ = 0° right / 180° left)
        let (p1_r, p2_r, gem_r, sh_r) = evaluate_directional_gain_dbi(
            spec,
            target_freq_ghz,
            target_pt.total_efficiency,
            ang_rad,
            0.0,
        );
        roll_bank_cut.push(PolarLobePoint {
            angle_deg: ang_deg,
            port1_gain_dbi: p1_r,
            port2_gain_dbi: p2_r,
            gemini_combined_gain_dbi: gem_r,
            cfrp_shadow_loss_db: sh_r,
        });
    }

    // 5. Generate 3D Spherical Lobe Rings (`13 × 24`) and collect spherical gain distribution
    let n_theta = 13usize;
    let n_phi = 24usize;
    let mut lobe_3d_rings = Vec::with_capacity(n_theta);
    let mut all_gains = Vec::with_capacity(n_theta * n_phi);

    // Anchor of the antenna pod in drone coordinates (rear tail `[0, -68 - standoff, 12 + elev]`)
    let pod_y = -62.0 - spec.cfrp_standoff_mm;
    let pod_z = 8.0 + spec.elevation_above_plate_mm;

    for it in 0..n_theta {
        let theta_deg = 15.0 + (it as f64) * (150.0 / ((n_theta - 1) as f64));
        let theta_rad = theta_deg.to_radians();
        let mut ring = Vec::with_capacity(n_phi);
        for ip in 0..n_phi {
            let phi_deg = (ip as f64) * (360.0 / (n_phi as f64));
            let phi_rad = phi_deg.to_radians();
            let (_, _, gem_dbi, _) = evaluate_directional_gain_dbi(
                spec,
                target_freq_ghz,
                target_pt.total_efficiency,
                theta_rad,
                phi_rad,
            );
            all_gains.push(gem_dbi);
            // Map gain in [-18 dBi, +5 dBi] to radius [18 mm, 115 mm] around the drone
            let norm_g = ((gem_dbi + 18.0) / 23.0).clamp(0.12, 1.15);
            let r_mm = 22.0 + 88.0 * norm_g;
            let x = r_mm * theta_rad.sin() * phi_rad.cos();
            let y = pod_y * 0.35 + r_mm * theta_rad.sin() * phi_rad.sin();
            let z = pod_z * 0.35 + r_mm * theta_rad.cos();
            ring.push(Lobe3dVertex {
                theta_deg,
                phi_deg,
                gain_dbi: gem_dbi,
                radius_mm: r_mm,
                pos_mm: [x, y, z],
            });
        }
        lobe_3d_rings.push(ring);
    }

    all_gains.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let peak_gain_dbi = all_gains.last().copied().unwrap_or(0.0);
    let min_null_gain_dbi = all_gains.first().copied().unwrap_or(-20.0);
    let p10_idx = ((all_gains.len() as f64) * 0.10).floor() as usize;
    let p10_blind_spot_floor_dbi = all_gains.get(p10_idx).copied().unwrap_or(min_null_gain_dbi);

    // Front (+Y, φ = 90° -> index 18 in 72-step yaw cut) vs Rear (-Y, φ = 270° -> index 54)
    let g_front = yaw_horizon_cut
        .get(18)
        .map(|p| p.gemini_combined_gain_dbi)
        .unwrap_or(0.0);
    let g_rear = yaw_horizon_cut
        .get(54)
        .map(|p| p.gemini_combined_gain_dbi)
        .unwrap_or(0.0);
    let front_to_back_db = g_rear - g_front;

    // Friis Link Budget Range (km) for ExpressLRS 250 Hz / Lora mode (`RX_sens = -108 dBm` at 2.4 GHz, `-115 dBm` at 900 MHz)
    // using the p10 blind-spot gain floor (so the link survives banked turns) with 12 dB fade margin.
    let tx_dbm = 10.0 * spec.tx_power_mw.max(1.0).log10();
    let rx_sens_dbm = if target_freq_ghz < 1.5 { -115.0 } else { -108.0 };
    let g_tx_ground_dbi = 3.5;
    let fade_margin_db = 14.0;
    let max_fspl_db = tx_dbm + g_tx_ground_dbi + p10_blind_spot_floor_dbi - rx_sens_dbm - fade_margin_db;
    // FSPL(dB) = 20 log10(d_km) + 20 log10(f_MHz) + 32.44
    let f_mhz = target_freq_ghz * 1000.0;
    let log10_d_km = (max_fspl_db - 20.0 * f_mhz.log10() - 32.44) / 20.0;
    let estimated_los_range_km = 10.0_f64.powf(log10_d_km).clamp(0.15, 65.0);

    BandPerformanceSummary {
        band_label: band_label.to_string(),
        target_freq_ghz,
        resonant_freq_ghz: res_pt.freq_ghz,
        freq_detuning_mhz: (res_pt.freq_ghz - target_freq_ghz) * 1000.0,
        z_at_target_ohm: [target_pt.z_real_ohm, target_pt.z_imag_ohm],
        gamma_at_target: [target_pt.gamma_re, target_pt.gamma_im],
        s11_at_target_db: target_pt.s11_db,
        vswr_at_target: target_pt.vswr,
        s21_isolation_db: target_pt.s21_db,
        envelope_correlation_ecc: ecc,
        bandwidth_s11_below_10db_mhz: bandwidth_mhz,
        radiation_efficiency_pct: target_pt.radiation_efficiency * 100.0,
        total_efficiency_pct: target_pt.total_efficiency * 100.0,
        cfrp_power_loss_pct: target_pt.cfrp_absorption_fraction * 100.0,
        peak_gain_dbi,
        p10_blind_spot_floor_dbi,
        min_null_gain_dbi,
        front_to_back_db,
        estimated_los_range_km,
        yaw_horizon_cut,
        pitch_elevation_cut,
        roll_bank_cut,
        lobe_3d_rings,
    }
}

/// Compute the analytical L-network match (`L_series` in nH, `C_shunt` in pF) that transforms
/// the unmatched input impedance at `target_low_freq_ghz` and `target_high_freq_ghz` closest to `50 Ω`.
pub fn suggest_l_network_matching(spec: &DroneAntennaDesignSpec) -> (f64, f64) {
    let mut raw_spec = spec.clone();
    raw_spec.match_series_nh = 0.0;
    raw_spec.match_shunt_pf = 0.0;
    let pt_low = evaluate_vna_point(&raw_spec, spec.target_low_freq_ghz, false);
    let pt_high = evaluate_vna_point(&raw_spec, spec.target_high_freq_ghz, true);

    let mut best_l = spec.match_series_nh;
    let mut best_c = spec.match_shunt_pf;
    let mut best_score = f64::INFINITY;

    for il in -15..=35 {
        let l_nh = (il as f64) * 0.25;
        for ic in 0..=28 {
            let c_pf = (ic as f64) * 0.10;
            let (r1, x1) = apply_l_network_match(
                pt_low.z_real_ohm,
                pt_low.z_imag_ohm,
                spec.target_low_freq_ghz,
                l_nh,
                c_pf,
            );
            let (r2, x2) = apply_l_network_match(
                pt_high.z_real_ohm,
                pt_high.z_imag_ohm,
                spec.target_high_freq_ghz,
                l_nh,
                c_pf,
            );
            let (_, _, g1) = complex_gamma_50(r1, x1);
            let (_, _, g2) = complex_gamma_50(r2, x2);
            let score = g1.max(g2) + 0.5 * (g1 + g2);
            if score < best_score {
                best_score = score;
                best_l = l_nh;
                best_c = c_pf;
            }
        }
    }
    (best_l, best_c)
}

/// Run a full dual-band VNA frequency sweep (`820–980 MHz` + `2300–2580 MHz`), Smith Chart trajectory,
/// and 3D radiation lobe analysis for a 4-inch DeadCat drone with an integrated Gemini antenna.
pub fn analyze_drone_antenna(spec: &DroneAntennaDesignSpec) -> DroneAntennaAnalysisReport {
    let n_pts = 41usize;
    let mut low_band_sweep = Vec::with_capacity(n_pts);
    for i in 0..n_pts {
        let f = 0.820 + (i as f64) * (0.160 / ((n_pts - 1) as f64));
        low_band_sweep.push(evaluate_vna_point(spec, f, false));
    }

    let mut high_band_sweep = Vec::with_capacity(n_pts);
    for i in 0..n_pts {
        let f = 2.300 + (i as f64) * (0.280 / ((n_pts - 1) as f64));
        high_band_sweep.push(evaluate_vna_point(spec, f, true));
    }

    let low_band_summary = summarise_band(
        spec,
        &low_band_sweep,
        spec.target_low_freq_ghz,
        "Gemini Low-Band (868 / 915 MHz ELRS)",
    );
    let high_band_summary = summarise_band(
        spec,
        &high_band_sweep,
        spec.target_high_freq_ghz,
        "Gemini High-Band (2.40–2.48 GHz ELRS)",
    );

    let (suggested_match_series_nh, suggested_match_shunt_pf) = suggest_l_network_matching(spec);

    // Multi-objective cost function J:
    // - S11 <= -12 dB in both 900 MHz and 2.4 GHz bands
    // - S21 <= -15 dB port-to-port isolation
    // - Radiation efficiency >= 72% (low CFRP eddy absorption)
    // - Blind-spot p10 gain floor >= -5.5 dBi
    let s11_worst = low_band_summary
        .s11_at_target_db
        .max(high_band_summary.s11_at_target_db);
    let s21_worst = low_band_summary
        .s21_isolation_db
        .max(high_band_summary.s21_isolation_db);
    let eff_min = low_band_summary
        .total_efficiency_pct
        .min(high_band_summary.total_efficiency_pct);
    let floor_min = low_band_summary
        .p10_blind_spot_floor_dbi
        .min(high_band_summary.p10_blind_spot_floor_dbi);

    let detune_penalty = 0.08
        * (low_band_summary.freq_detuning_mhz.abs() + high_band_summary.freq_detuning_mhz.abs());
    let composite_cost_j = (s11_worst + 14.0) * 1.8
        + (s21_worst + 16.0).max(0.0) * 1.4
        + (75.0 - eff_min).max(0.0) * 0.45
        + (-4.5 - floor_min).max(0.0) * 2.2
        + detune_penalty;

    let all_targets_met = low_band_summary.s11_at_target_db <= -10.0
        && high_band_summary.s11_at_target_db <= -10.0
        && s21_worst <= -15.0
        && eff_min >= 65.0
        && floor_min >= -6.5;

    let mut notes = Vec::new();
    if low_band_summary.cfrp_power_loss_pct > 20.0 || high_band_summary.cfrp_power_loss_pct > 20.0
    {
        notes.push(format!(
            "High CFRP Eddy-Current Loss ({:.1}% at 915 MHz, {:.1}% at 2.44 GHz): the antenna sits only {:.1} mm from the conductive carbon plate. Increase CFRP standoff d_cfrp ≥ 20 mm and elevation ≥ 14 mm.",
            low_band_summary.cfrp_power_loss_pct,
            high_band_summary.cfrp_power_loss_pct,
            spec.cfrp_standoff_mm
        ));
    }
    if low_band_summary.freq_detuning_mhz.abs() > 15.0
        || high_band_summary.freq_detuning_mhz.abs() > 35.0
    {
        notes.push(format!(
            "Dielectric Pod Detuning: TPU wall ({:.1} mm, ε_r = {:.2}) shifts resonance by {:+.0} MHz (low) and {:+.0} MHz (high). Trim L_900 and L_2400 to pre-compensate for 1/√ε_eff wave slowing.",
            spec.dielectric_pod_wall_mm,
            spec.pod_eps_r,
            low_band_summary.freq_detuning_mhz,
            high_band_summary.freq_detuning_mhz
        ));
    }
    if spec.diversity_skew_deg < 65.0 || spec.diversity_skew_deg > 115.0 {
        notes.push(format!(
            "Sub-Orthogonal Diversity Skew ({:.0}°): Port 1 and Port 2 cross-couple (S₂₁ = {:.1} dB, ECC = {:.3}). Spread the Gemini V-arms toward 85°–92° for orthogonal polarisation diversity.",
            spec.diversity_skew_deg, s21_worst, low_band_summary.envelope_correlation_ecc
        ));
    }
    if all_targets_met {
        notes.push(format!(
            "Dual-Band Gemini Qualification PASS: 915 MHz S₁₁ = {:.1} dB (VSWR {:.2}, η = {:.1}%), 2.44 GHz S₁₁ = {:.1} dB (VSWR {:.2}, η = {:.1}%), Isolation S₂₁ = {:.1} dB, p10 Blind-Spot Floor = {:.1} dBi.",
            low_band_summary.s11_at_target_db,
            low_band_summary.vswr_at_target,
            low_band_summary.total_efficiency_pct,
            high_band_summary.s11_at_target_db,
            high_band_summary.vswr_at_target,
            high_band_summary.total_efficiency_pct,
            s21_worst,
            floor_min
        ));
    }

    DroneAntennaAnalysisReport {
        spec: spec.clone(),
        low_band_sweep,
        high_band_sweep,
        low_band_summary,
        high_band_summary,
        suggested_match_series_nh,
        suggested_match_shunt_pf,
        composite_cost_j,
        all_targets_met,
        engineering_notes: notes,
    }
}

/// Optimise the 4-inch DeadCat Gemini Dual-Band Antenna geometry (`cfrp_standoff_mm`,
/// `elevation_above_plate_mm`, `low_band_arm_length_mm`, `high_band_arm_length_mm`,
/// `diversity_skew_deg`, `dielectric_pod_wall_mm`, and L-network `match_series_nh` / `match_shunt_pf`)
/// to simultaneously center both `900 MHz` and `2.4 GHz` on the `50 Ω` Smith Chart, minimize CFRP
/// eddy-current loss, and eliminate nose-on/banked radiation blind spots.
pub fn optimise_drone_antenna(
    initial: &DroneAntennaDesignSpec,
    max_iters: usize,
) -> DroneAntennaOptimisationReport {
    let initial_report = analyze_drone_antenna(initial);
    let mut cur = initial.clone();

    // Stage 1: Coordinate pattern search over physical geometry & dielectric pod parameters
    let iters = max_iters.clamp(1, 25);
    for _ in 0..iters {
        // 1a. Analytical frequency-ratio length correction for L_900 and L_2400
        let mut no_match = cur.clone();
        no_match.match_series_nh = 0.0;
        no_match.match_shunt_pf = 0.0;
        let rep = analyze_drone_antenna(&no_match);

        let ratio_low = (rep.low_band_summary.resonant_freq_ghz / cur.target_low_freq_ghz).clamp(0.82, 1.22);
        cur.low_band_arm_length_mm = (cur.low_band_arm_length_mm * ratio_low).clamp(105.0, 175.0);

        let ratio_high = (rep.high_band_summary.resonant_freq_ghz / cur.target_high_freq_ghz).clamp(0.82, 1.22);
        cur.high_band_arm_length_mm = (cur.high_band_arm_length_mm * ratio_high).clamp(38.0, 68.0);

        // 1b. Step CFRP standoff, elevation, pod wall, and diversity skew along descent directions
        let candidates = [
            DroneAntennaDesignSpec {
                cfrp_standoff_mm: (cur.cfrp_standoff_mm + 4.5).clamp(5.0, 34.0),
                elevation_above_plate_mm: (cur.elevation_above_plate_mm + 3.0).clamp(2.0, 26.0),
                ..cur.clone()
            },
            DroneAntennaDesignSpec {
                diversity_skew_deg: (cur.diversity_skew_deg + 14.0).clamp(30.0, 92.0),
                dielectric_pod_wall_mm: (cur.dielectric_pod_wall_mm - 0.35).clamp(1.2, 4.0),
                ..cur.clone()
            },
            DroneAntennaDesignSpec {
                diversity_skew_deg: 88.0,
                cfrp_standoff_mm: cur.cfrp_standoff_mm.max(23.5),
                elevation_above_plate_mm: cur.elevation_above_plate_mm.max(16.5),
                dielectric_pod_wall_mm: cur.dielectric_pod_wall_mm.min(1.6),
                ..cur.clone()
            },
        ];

        let mut best_cand = cur.clone();
        let mut best_cost = analyze_drone_antenna(&cur).composite_cost_j;
        for mut cand in candidates {
            let (l_nh, c_pf) = suggest_l_network_matching(&cand);
            cand.match_series_nh = l_nh;
            cand.match_shunt_pf = c_pf;
            let c_rep = analyze_drone_antenna(&cand);
            if c_rep.composite_cost_j < best_cost {
                best_cost = c_rep.composite_cost_j;
                best_cand = cand;
            }
        }
        cur = best_cand;

        // Re-centre lengths around exact reactance zero-crossing
        for &delta_l in &[-1.8, -0.6, 0.6, 1.8] {
            let mut trial = cur.clone();
            trial.low_band_arm_length_mm = (cur.low_band_arm_length_mm + delta_l).clamp(105.0, 175.0);
            let (l_nh, c_pf) = suggest_l_network_matching(&trial);
            trial.match_series_nh = l_nh;
            trial.match_shunt_pf = c_pf;
            if analyze_drone_antenna(&trial).composite_cost_j < analyze_drone_antenna(&cur).composite_cost_j {
                cur = trial;
            }
        }
        for &delta_h in &[-0.9, -0.3, 0.3, 0.9] {
            let mut trial = cur.clone();
            trial.high_band_arm_length_mm = (cur.high_band_arm_length_mm + delta_h).clamp(38.0, 68.0);
            let (l_nh, c_pf) = suggest_l_network_matching(&trial);
            trial.match_series_nh = l_nh;
            trial.match_shunt_pf = c_pf;
            if analyze_drone_antenna(&trial).composite_cost_j < analyze_drone_antenna(&cur).composite_cost_j {
                cur = trial;
            }
        }
    }

    let (l_nh, c_pf) = suggest_l_network_matching(&cur);
    cur.match_series_nh = l_nh;
    cur.match_shunt_pf = c_pf;

    let optimised_report = analyze_drone_antenna(&cur);
    let fsm_3d = build_deadcat_drone_fsm(&cur);

    DroneAntennaOptimisationReport {
        initial_report,
        optimised_report,
        iterations: iters,
        fsm_3d,
    }
}

/// Generate a complete, parametric 3D `.fsm` CAD assembly for the **4-Inch DeadCat FPV Drone**
/// with its CFRP bottom/top plates, swept-forward front arms, rear arms, `4 × 1404` brushless motors,
/// `20×20 mm` FC/ESC stack, Aluminum FPV camera cage, top-mounted `4S 850 mAh` LiPo battery, and
/// the **3D-Printed TPU Gemini Dual-Band (`900 MHz` + `2.4 GHz`) Antenna Mount & Dipole Elements**.
pub fn build_deadcat_drone_fsm(spec: &DroneAntennaDesignSpec) -> String {
    let wb = spec.wheelbase_mm.clamp(140.0, 220.0);
    let arm_r = 0.5 * wb; // 86 mm from center to motor shaft
    let f_ang = spec.front_arm_sweep_deg.to_radians();
    let r_ang = spec.rear_arm_sweep_deg.to_radians();

    // Front motor coordinates (wide forward-swept DeadCat geometry clears props from camera FOV)
    let fm_x = arm_r * f_ang.cos();
    let fm_y = 20.0 + (arm_r - 12.0) * f_ang.sin();

    // Rear motor coordinates
    let rm_x = arm_r * r_ang.cos();
    let rm_y = -18.0 - (arm_r - 10.0) * r_ang.sin();

    // Arm centers and lengths
    let f_arm_cx = 0.5 * (14.0 + fm_x);
    let f_arm_cy = 0.5 * (20.0 + fm_y);
    let f_arm_len = ((fm_x - 14.0).powi(2) + (fm_y - 20.0).powi(2)).sqrt() + 14.0;
    let f_arm_rot_z = (fm_y - 20.0).atan2(fm_x - 14.0).to_degrees();

    let r_arm_cx = 0.5 * (12.0 + rm_x);
    let r_arm_cy = 0.5 * (-18.0 + rm_y);
    let r_arm_len = ((rm_x - 12.0).powi(2) + (rm_y + 18.0).powi(2)).sqrt() + 14.0;
    let r_arm_rot_z = (-18.0 - rm_y).atan2(rm_x - 12.0).to_degrees();

    // Antenna TPU Pod & Gemini Dual-Band Element positions
    let pod_y = -58.0 - spec.cfrp_standoff_mm;
    let pod_z = 8.0 + spec.elevation_above_plate_mm;
    let arm_900_half = 0.5 * spec.low_band_arm_length_mm;
    let arm_2400_half = 0.5 * spec.high_band_arm_length_mm;
    let half_skew = 0.5 * spec.diversity_skew_deg;

    let rep = analyze_drone_antenna(spec);

    format!(
        r##"fieldsmith 1
name "4-Inch DeadCat FPV Drone + Gemini Dual-Band (900 MHz / 2.4 GHz) Antenna"
material cf_woven_3k

let wheelbase_mm = {wb:.1}
let cfrp_standoff_mm = {standoff:.2}
let antenna_elev_mm = {elev:.2}
let gemini_l900_mm = {l900:.2}
let gemini_l2400_mm = {l2400:.2}
let gemini_skew_deg = {skew:.1}
let tpu_pod_wall_mm = {wall:.2}
let match_series_nh = {l_nh:.2}
let match_shunt_pf = {c_pf:.2}
let vna_s11_915_db = {s11_915:.2}
let vna_s11_2440_db = {s11_2440:.2}
let vna_eff_915_pct = {eff_915:.1}
let vna_eff_2440_pct = {eff_2440:.1}

group cfrp_deadcat_airframe {{
  box cfrp_bottom_plate size=(34, 118, 2.0) round=3 at=(0, 0, 0)
  box cfrp_top_plate size=(30, 108, 1.5) round=3 at=(0, -2, 20.0)
  box arm_front_right size=({f_arm_len:.1}, 9.5, 3.5) round=1.2 at=({f_arm_cx:.1}, {f_arm_cy:.1}, -1.0) rotate=(0, 0, {f_arm_rot_z:.1})
  box arm_front_left size=({f_arm_len:.1}, 9.5, 3.5) round=1.2 at=(-{f_arm_cx:.1}, {f_arm_cy:.1}, -1.0) rotate=(0, 0, -{f_arm_rot_z:.1})
  box arm_rear_right size=({r_arm_len:.1}, 9.0, 3.5) round=1.2 at=({r_arm_cx:.1}, {r_arm_cy:.1}, -1.0) rotate=(0, 0, -{r_arm_rot_z:.1})
  box arm_rear_left size=({r_arm_len:.1}, 9.0, 3.5) round=1.2 at=(-{r_arm_cx:.1}, {r_arm_cy:.1}, -1.0) rotate=(0, 0, {r_arm_rot_z:.1})
  cylinder standoff_fr r=2.2 h=18.5 at=(12, 34, 10)
  cylinder standoff_fl r=2.2 h=18.5 at=(-12, 34, 10)
  cylinder standoff_rr r=2.2 h=18.5 at=(12, -48, 10)
  cylinder standoff_rl r=2.2 h=18.5 at=(-12, -48, 10)
}}
color "#1E293B" on cfrp_deadcat_airframe
color "#334155" on cfrp_top_plate
material ti64 on standoff_fr
material ti64 on standoff_fl
material ti64 on standoff_rr
material ti64 on standoff_rl
color "#94A3B8" on standoff_fr
color "#94A3B8" on standoff_fl
color "#94A3B8" on standoff_rr
color "#94A3B8" on standoff_rl

group propulsion_motors_1404 {{
  cylinder motor_fr_bell r=9.2 h=11.5 round=1.0 at=({fm_x:.1}, {fm_y:.1}, 6.0)
  cylinder motor_fl_bell r=9.2 h=11.5 round=1.0 at=(-{fm_x:.1}, {fm_y:.1}, 6.0)
  cylinder motor_rr_bell r=9.2 h=11.5 round=1.0 at=({rm_x:.1}, {rm_y:.1}, 6.0)
  cylinder motor_rl_bell r=9.2 h=11.5 round=1.0 at=(-{rm_x:.1}, {rm_y:.1}, 6.0)
  cylinder prop_disc_fr r=50.8 h=1.2 at=({fm_x:.1}, {fm_y:.1}, 13.0)
  cylinder prop_disc_fl r=50.8 h=1.2 at=(-{fm_x:.1}, {fm_y:.1}, 13.0)
  cylinder prop_disc_rr r=50.8 h=1.2 at=({rm_x:.1}, {rm_y:.1}, 13.0)
  cylinder prop_disc_rl r=50.8 h=1.2 at=(-{rm_x:.1}, {rm_y:.1}, 13.0)
}}
material ss316l on motor_fr_bell
material ss316l on motor_fl_bell
material ss316l on motor_rr_bell
material ss316l on motor_rl_bell
color "#475569" on motor_fr_bell
color "#475569" on motor_fl_bell
color "#475569" on motor_rr_bell
color "#475569" on motor_rl_bell
color "#38BDF8" on prop_disc_fr
color "#38BDF8" on prop_disc_fl
color "#38BDF8" on prop_disc_rr
color "#38BDF8" on prop_disc_rl

group avionics_and_battery {{
  box esc_board_20x20 size=(26, 26, 1.8) round=1.5 at=(0, -4, 5.5)
  box fc_board_20x20 size=(25, 25, 1.6) round=1.5 at=(0, -4, 11.0)
  box elrs_gemini_rx_pcb size=(18, 22, 1.6) round=1.0 at=(0, -32, 8.5)
  box fpv_cam_cage_al7075 size=(21, 19, 19) round=2.0 at=(0, 48, 10.0)
  cylinder fpv_cam_lens r=7.0 h=10.0 at=(0, 56, 10.0) rotate=(90, 0, 0)
  box lipo_4s_850mah size=(30, 58, 24) round=3.0 at=(0, -4, 33.0)
}}
color "#059669" on esc_board_20x20
color "#10B981" on fc_board_20x20
color "#2563EB" on elrs_gemini_rx_pcb
material alsi10mg on fpv_cam_cage_al7075
color "#94A3B8" on fpv_cam_cage_al7075
color "#0284C7" on fpv_cam_lens
material petg on lipo_4s_850mah
color "#F59E0B" on lipo_4s_850mah

group gemini_dual_band_antenna_assy {{
  box tpu_standoff_boom size=(14, {boom_len:.1}, 7.0) round=2.5 at=(0, {boom_y:.1}, {boom_z:.1}) rotate=(-14, 0, 0)
  box tpu_dipole_crossbar size=(26, 9.0, 9.0) round=3.0 at=(0, {pod_y:.1}, {pod_z:.1})
  cylinder coax_feed_port1 r=1.3 h={coax_len:.1} at=(4.5, {boom_y:.1}, {boom_z:.1}) rotate=(76, 0, 0)
  cylinder coax_feed_port2 r=1.3 h={coax_len:.1} at=(-4.5, {boom_y:.1}, {boom_z:.1}) rotate=(76, 0, 0)
  cylinder gemini_p1_900mhz_dipole r=1.4 h={l900:.1} round=0.6 at=(6.0, {pod_y:.1}, {pod_z:.1}) rotate=(-14, {half_skew:.1}, 0)
  cylinder gemini_p1_2400mhz_sleeve r=2.3 h={l2400:.1} round=0.8 at=(6.0, {pod_y:.1}, {pod_z:.1}) rotate=(-14, {half_skew:.1}, 0)
  cylinder gemini_p2_900mhz_dipole r=1.4 h={l900:.1} round=0.6 at=(-6.0, {pod_y:.1}, {pod_z:.1}) rotate=(-14, -{half_skew:.1}, 0)
  cylinder gemini_p2_2400mhz_sleeve r=2.3 h={l2400:.1} round=0.8 at=(-6.0, {pod_y:.1}, {pod_z:.1}) rotate=(-14, -{half_skew:.1}, 0)
  cylinder tpu_boot_cap_p1 r={boot_r:.1} h={arm_900_half:.1} round=1.2 at=(6.0, {pod_y:.1}, {pod_z:.1}) rotate=(-14, {half_skew:.1}, 0)
  cylinder tpu_boot_cap_p2 r={boot_r:.1} h={arm_900_half:.1} round=1.2 at=(-6.0, {pod_y:.1}, {pod_z:.1}) rotate=(-14, -{half_skew:.1}, 0)
}}
material pa12 on tpu_standoff_boom
material pa12 on tpu_dipole_crossbar
material pa12 on tpu_boot_cap_p1
material pa12 on tpu_boot_cap_p2
material cucrzr on coax_feed_port1
material cucrzr on coax_feed_port2
material cucrzr on gemini_p1_900mhz_dipole
material cucrzr on gemini_p1_2400mhz_sleeve
material cucrzr on gemini_p2_900mhz_dipole
material cucrzr on gemini_p2_2400mhz_sleeve
color "#0EA5E9" on tpu_standoff_boom
color "#0284C7" on tpu_dipole_crossbar
color "#D97706" on coax_feed_port1
color "#D97706" on coax_feed_port2
color "#F59E0B" on gemini_p1_900mhz_dipole
color "#FDE047" on gemini_p1_2400mhz_sleeve
color "#F59E0B" on gemini_p2_900mhz_dipole
color "#34D399" on gemini_p2_2400mhz_sleeve
color "#38BDF8" on tpu_boot_cap_p1
color "#38BDF8" on tpu_boot_cap_p2

region rear_standoff_mount = cfrp_bottom_plate.nz
region top_deck = cfrp_top_plate.pz
support bench_fix rear_standoff_mount fixed=xyz
load prop_thrust top_deck force=(0, 0, 18)
"##,
        wb = wb,
        standoff = spec.cfrp_standoff_mm,
        elev = spec.elevation_above_plate_mm,
        l900 = spec.low_band_arm_length_mm,
        l2400 = spec.high_band_arm_length_mm,
        skew = spec.diversity_skew_deg,
        wall = spec.dielectric_pod_wall_mm,
        l_nh = spec.match_series_nh,
        c_pf = spec.match_shunt_pf,
        s11_915 = rep.low_band_summary.s11_at_target_db,
        s11_2440 = rep.high_band_summary.s11_at_target_db,
        eff_915 = rep.low_band_summary.total_efficiency_pct,
        eff_2440 = rep.high_band_summary.total_efficiency_pct,
        f_arm_len = f_arm_len,
        f_arm_cx = f_arm_cx,
        f_arm_cy = f_arm_cy,
        f_arm_rot_z = f_arm_rot_z,
        r_arm_len = r_arm_len,
        r_arm_cx = r_arm_cx,
        r_arm_cy = r_arm_cy,
        r_arm_rot_z = r_arm_rot_z,
        fm_x = fm_x,
        fm_y = fm_y,
        rm_x = rm_x,
        rm_y = rm_y,
        boom_len = spec.cfrp_standoff_mm + 16.0,
        boom_y = -50.0 - 0.5 * spec.cfrp_standoff_mm,
        boom_z = 10.0 + 0.5 * spec.elevation_above_plate_mm,
        pod_y = pod_y,
        pod_z = pod_z,
        coax_len = spec.cfrp_standoff_mm + 20.0,
        half_skew = half_skew,
        boot_r = 1.4 + spec.dielectric_pod_wall_mm,
        arm_900_half = arm_900_half.max(arm_2400_half + 10.0),
    )
}

// ============================================================================
// CoupledEngineeringSystem Implementation (`deadcat_4in_gemini_antenna_v1`)
// ============================================================================

pub const DRONE_ANTENNA_N_STATIONS: usize = 10;
pub const DRONE_ANTENNA_N_FIELDS: usize = 4;
pub const DRONE_ANTENNA_N_DOFS: usize = DRONE_ANTENNA_N_STATIONS * DRONE_ANTENNA_N_FIELDS;

#[derive(Clone, Debug, Default)]
pub struct DeadCatGeminiAntennaSystem;

impl DeadCatGeminiAntennaSystem {
    pub fn new() -> Self {
        Self
    }

    pub fn spec_from_params(&self, p: &[f64], theta: &[f64]) -> DroneAntennaDesignSpec {
        DroneAntennaDesignSpec {
            topology: DroneAntennaTopology::RearTpuVDipolePod,
            target_low_freq_ghz: 0.915,
            target_high_freq_ghz: 2.440,
            cfrp_standoff_mm: p.first().copied().unwrap_or(24.0),
            low_band_arm_length_mm: p.get(1).copied().unwrap_or(136.5),
            high_band_arm_length_mm: p.get(2).copied().unwrap_or(51.2),
            diversity_skew_deg: p.get(3).copied().unwrap_or(86.0),
            elevation_above_plate_mm: p.get(4).copied().unwrap_or(16.0),
            dielectric_pod_wall_mm: p.get(5).copied().unwrap_or(1.8),
            match_series_nh: p.get(6).copied().unwrap_or(1.8),
            match_shunt_pf: p.get(7).copied().unwrap_or(0.65),
            cfrp_conductivity_s_per_m: theta.first().copied().unwrap_or(15_000.0),
            pod_eps_r: theta.get(1).copied().unwrap_or(2.75),
            pod_loss_tan: theta.get(2).copied().unwrap_or(0.014),
            wheelbase_mm: 172.0,
            front_arm_sweep_deg: 18.0,
            rear_arm_sweep_deg: 34.0,
            tx_power_mw: 100.0,
        }
    }
}

impl CoupledEngineeringSystem for DeadCatGeminiAntennaSystem {
    fn system_id(&self) -> &str {
        "deadcat_4in_gemini_antenna_v1"
    }

    fn system_title(&self) -> &str {
        "4-Inch DeadCat CFRP Drone + Gemini Dual-Band (900 MHz / 2.4 GHz) Antenna VNA, Smith Chart & 3D Lobe System"
    }

    fn industry_sector(&self) -> &str {
        "Unmanned Aerial Vehicles (FPV / Long-Range Sub-250g Carbon-Fibre Composites & Dual-Band RF Diversity)"
    }

    fn field_descriptors(&self) -> Vec<FieldDescriptor> {
        vec![
            FieldDescriptor {
                name: "dipole_current_900mhz_ma".into(),
                symbol: "I_900(s)".into(),
                unit: "mA".into(),
                reference_scale: 45.0,
                domain: "low_band_dipole_element".into(),
            },
            FieldDescriptor {
                name: "dipole_current_2400mhz_ma".into(),
                symbol: "I_2400(s)".into(),
                unit: "mA".into(),
                reference_scale: 45.0,
                domain: "high_band_sleeve_element".into(),
            },
            FieldDescriptor {
                name: "cfrp_eddy_surface_current_a_per_m".into(),
                symbol: "J_cfrp(s)".into(),
                unit: "A/m".into(),
                reference_scale: 1.5,
                domain: "deadcat_carbon_airframe".into(),
            },
            FieldDescriptor {
                name: "azimuthal_gain_envelope_dbi".into(),
                symbol: "G_div(φ)".into(),
                unit: "dBi".into(),
                reference_scale: 3.0,
                domain: "far_field_horizon_sphere".into(),
            },
        ]
    }

    fn num_dofs(&self) -> usize {
        DRONE_ANTENNA_N_DOFS
    }

    fn design_parameters(&self) -> Vec<DesignParameter> {
        vec![
            DesignParameter {
                name: "cfrp_standoff_mm".into(),
                symbol: "d_cfrp".into(),
                unit: "mm".into(),
                category: DesignParameterCategory::GeometryContour,
                nominal: 24.0,
                min_bound: 4.0,
                max_bound: 40.0,
                fd_step: 1.0e-2,
                cost_rate_per_unit: 0.05,
                description: "Standoff distance from rear DeadCat CFRP plate & motor bells to Gemini dipole feed".into(),
            },
            DesignParameter {
                name: "low_band_arm_length_mm".into(),
                symbol: "L_900".into(),
                unit: "mm".into(),
                category: DesignParameterCategory::GeometryContour,
                nominal: 136.5,
                min_bound: 105.0,
                max_bound: 175.0,
                fd_step: 1.0e-2,
                cost_rate_per_unit: 0.01,
                description: "Total tip-to-tip active length of 900 MHz low-band dipole element (dielectric pre-compensated)".into(),
            },
            DesignParameter {
                name: "high_band_arm_length_mm".into(),
                symbol: "L_2400".into(),
                unit: "mm".into(),
                category: DesignParameterCategory::GeometryContour,
                nominal: 51.2,
                min_bound: 38.0,
                max_bound: 68.0,
                fd_step: 1.0e-2,
                cost_rate_per_unit: 0.01,
                description: "Total tip-to-tip active length of 2.4 GHz high-band sleeve dipole element".into(),
            },
            DesignParameter {
                name: "diversity_skew_deg".into(),
                symbol: "α_skew".into(),
                unit: "deg".into(),
                category: DesignParameterCategory::ComponentHardware,
                nominal: 86.0,
                min_bound: 25.0,
                max_bound: 110.0,
                fd_step: 1.0e-2,
                cost_rate_per_unit: 0.0,
                description: "Included angle between Gemini Port 1 and Port 2 diversity dipole arms (90° = orthogonal)".into(),
            },
            DesignParameter {
                name: "elevation_above_plate_mm".into(),
                symbol: "z_pod".into(),
                unit: "mm".into(),
                category: DesignParameterCategory::GeometryContour,
                nominal: 16.0,
                min_bound: 2.0,
                max_bound: 30.0,
                fd_step: 1.0e-2,
                cost_rate_per_unit: 0.03,
                description: "Elevation of antenna feed above the DeadCat top carbon plate & LiPo shadow zone".into(),
            },
            DesignParameter {
                name: "dielectric_pod_wall_mm".into(),
                symbol: "t_tpu".into(),
                unit: "mm".into(),
                category: DesignParameterCategory::GeometryContour,
                nominal: 1.8,
                min_bound: 0.8,
                max_bound: 4.5,
                fd_step: 1.0e-3,
                cost_rate_per_unit: 0.02,
                description: "Wall thickness of the 3D-printed TPU/PA12 antenna boot surrounding the elements".into(),
            },
            DesignParameter {
                name: "match_series_nh".into(),
                symbol: "L_ser".into(),
                unit: "nH".into(),
                category: DesignParameterCategory::ActuatorSetpoint,
                nominal: 1.8,
                min_bound: -5.0,
                max_bound: 12.0,
                fd_step: 1.0e-3,
                cost_rate_per_unit: 0.0,
                description: "Feed L-network series matching inductance L_ser (nH)".into(),
            },
            DesignParameter {
                name: "match_shunt_pf".into(),
                symbol: "C_sh".into(),
                unit: "pF".into(),
                category: DesignParameterCategory::ActuatorSetpoint,
                nominal: 0.65,
                min_bound: 0.0,
                max_bound: 4.0,
                fd_step: 1.0e-3,
                cost_rate_per_unit: 0.0,
                description: "Feed L-network shunt matching capacitance C_sh (pF)".into(),
            },
        ]
    }

    fn epistemic_parameters(&self) -> Vec<EpistemicParameter> {
        vec![
            EpistemicParameter {
                name: "cfrp_inplane_conductivity_s_per_m".into(),
                symbol: "σ_cfrp".into(),
                unit: "S/m".into(),
                nominal: 15_000.0,
                std_dev_sigma: 2_500.0,
                min_bound: 5_000.0,
                max_bound: 35_000.0,
                fd_step: 10.0,
                physical_mechanism: "Anisotropic 3K woven carbon-fibre laminate conductivity & resin-rich surface skin scatter".into(),
            },
            EpistemicParameter {
                name: "tpu_pod_permittivity_eps_r".into(),
                symbol: "ε_r,pod".into(),
                unit: "—".into(),
                nominal: 2.75,
                std_dev_sigma: 0.18,
                min_bound: 2.1,
                max_bound: 3.6,
                fd_step: 1.0e-3,
                physical_mechanism: "3D-printed TPU / PA12 infill density & moisture-dependent relative permittivity".into(),
            },
            EpistemicParameter {
                name: "tpu_pod_loss_tangent".into(),
                symbol: "tan δ".into(),
                unit: "—".into(),
                nominal: 0.014,
                std_dev_sigma: 0.004,
                min_bound: 0.002,
                max_bound: 0.045,
                fd_step: 1.0e-4,
                physical_mechanism: "Dielectric loss tangent of polyurethane / polyamide print filament at 0.9–2.4 GHz".into(),
            },
        ]
    }

    fn objective_descriptors(&self) -> Vec<ObjectiveDescriptor> {
        vec![
            ObjectiveDescriptor {
                name: "s11_low_band_915mhz_db".into(),
                symbol: "S₁₁(915)".into(),
                unit: "dB".into(),
                kind: ObjectiveKind::UpperLimit(-10.0),
                description: "VNA Return Loss S₁₁ at 915 MHz Low-Band (must be ≤ -10 dB, VSWR < 1.92)".into(),
            },
            ObjectiveDescriptor {
                name: "s11_high_band_2440mhz_db".into(),
                symbol: "S₁₁(2440)".into(),
                unit: "dB".into(),
                kind: ObjectiveKind::UpperLimit(-10.0),
                description: "VNA Return Loss S₁₁ at 2.44 GHz High-Band (must be ≤ -10 dB, VSWR < 1.92)".into(),
            },
            ObjectiveDescriptor {
                name: "s21_diversity_isolation_db".into(),
                symbol: "S₂₁".into(),
                unit: "dB".into(),
                kind: ObjectiveKind::UpperLimit(-15.0),
                description: "Mutual coupling S₂₁ between Gemini Port 1 and Port 2 diversity chains (≤ -15 dB)".into(),
            },
            ObjectiveDescriptor {
                name: "min_total_radiation_efficiency_pct".into(),
                symbol: "η_tot".into(),
                unit: "%".into(),
                kind: ObjectiveKind::LowerLimit(65.0),
                description: "Minimum total antenna efficiency across both bands after CFRP eddy & mismatch losses (≥ 65%)".into(),
            },
            ObjectiveDescriptor {
                name: "p10_blind_spot_gain_floor_dbi".into(),
                symbol: "G_p10".into(),
                unit: "dBi".into(),
                kind: ObjectiveKind::LowerLimit(-6.0),
                description: "10th-percentile spherical radiation gain floor across all drone pitch/roll/yaw attitudes (≥ -6.0 dBi)".into(),
            },
            ObjectiveDescriptor {
                name: "estimated_gemini_los_range_km".into(),
                symbol: "R_LOS".into(),
                unit: "km".into(),
                kind: ObjectiveKind::Maximize,
                description: "Certified dual-band line-of-sight range at 100 mW with 14 dB fade margin in worst p10 attitude".into(),
            },
        ]
    }

    fn initial_state(&self, design_p: &[f64], epistemic_theta: &[f64]) -> Vec<f64> {
        let spec = self.spec_from_params(design_p, epistemic_theta);
        let pt_low = evaluate_vna_point(&spec, spec.target_low_freq_ghz, false);
        let pt_high = evaluate_vna_point(&spec, spec.target_high_freq_ghz, true);
        let i0_low_ma = (2.0 * spec.tx_power_mw / pt_low.z_real_ohm.max(5.0)).sqrt() * 1000.0;
        let i0_high_ma = (2.0 * spec.tx_power_mw / pt_high.z_real_ohm.max(5.0)).sqrt() * 1000.0;

        let mut w = vec![0.0; DRONE_ANTENNA_N_DOFS];
        for s in 0..DRONE_ANTENNA_N_STATIONS {
            let xi = (s as f64) / ((DRONE_ANTENNA_N_STATIONS - 1) as f64);
            let profile = (PI * xi).sin();
            let phi = 2.0 * PI * xi;
            let (_, _, gem_dbi, _) = evaluate_directional_gain_dbi(
                &spec,
                spec.target_high_freq_ghz,
                pt_high.total_efficiency,
                0.5 * PI,
                phi,
            );
            let base = s * DRONE_ANTENNA_N_FIELDS;
            w[base] = i0_low_ma * profile;
            w[base + 1] = i0_high_ma * profile;
            w[base + 2] = 0.025 * i0_low_ma * pt_low.cfrp_absorption_fraction * (1.0 - xi);
            w[base + 3] = gem_dbi;
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
        let target = self.initial_state(design_p, epistemic_theta);
        let mut r = vec![0.0; DRONE_ANTENNA_N_DOFS];
        for i in 0..DRONE_ANTENNA_N_DOFS {
            let transient = inv_dt * (w[i] - w_prev.get(i).copied().unwrap_or(w[i])) * 1.0e-6;
            r[i] = (w[i] - target[i]) + transient;
        }
        r
    }

    fn eval_objectives(
        &self,
        _state_w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<f64> {
        let spec = self.spec_from_params(design_p, epistemic_theta);
        let rep = analyze_drone_antenna(&spec);
        let s11_915 = rep.low_band_summary.s11_at_target_db;
        let s11_2440 = rep.high_band_summary.s11_at_target_db;
        let s21_worst = rep
            .low_band_summary
            .s21_isolation_db
            .max(rep.high_band_summary.s21_isolation_db);
        let eff_min = rep
            .low_band_summary
            .total_efficiency_pct
            .min(rep.high_band_summary.total_efficiency_pct);
        let floor_min = rep
            .low_band_summary
            .p10_blind_spot_floor_dbi
            .min(rep.high_band_summary.p10_blind_spot_floor_dbi);
        let range_min = rep
            .low_band_summary
            .estimated_los_range_km
            .min(rep.high_band_summary.estimated_los_range_km);

        vec![
            s11_915,
            s11_2440,
            s21_worst,
            eff_min,
            floor_min,
            range_min,
        ]
    }

    fn eval_conservation_audits(
        &self,
        _state_w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<ConservationAudit> {
        let spec = self.spec_from_params(design_p, epistemic_theta);
        let pt = evaluate_vna_point(&spec, spec.target_high_freq_ghz, true);
        let p_in_mw = spec.tx_power_mw;
        let p_refl_mw = p_in_mw * pt.gamma_mag.powi(2);
        let p_acc_mw = p_in_mw - p_refl_mw;
        let p_rad_mw = p_acc_mw * pt.radiation_efficiency;
        let p_loss_mw = p_acc_mw * (1.0 - pt.radiation_efficiency);
        let imbalance = (p_in_mw - (p_refl_mw + p_rad_mw + p_loss_mw)).abs();
        vec![ConservationAudit {
            name: "RF Port Poynting Power Balance (P_in = P_refl + P_rad + P_cfrp_loss)".into(),
            inflow: p_in_mw,
            outflow: p_rad_mw + p_refl_mw,
            source_or_sink: -p_loss_mw,
            net_imbalance: imbalance,
            unit: "mW".into(),
            relative_error: imbalance / p_in_mw.max(1e-6),
            tolerance: 1.0e-9,
        }]
    }

    fn eval_causal_attribution(
        &self,
        state_w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<CausalAttributionTrack> {
        let spec = self.spec_from_params(design_p, epistemic_theta);
        let mut points = Vec::with_capacity(DRONE_ANTENNA_N_STATIONS);
        for s in 0..DRONE_ANTENNA_N_STATIONS {
            let xi = (s as f64) / ((DRONE_ANTENNA_N_STATIONS - 1) as f64);
            let base = s * DRONE_ANTENNA_N_FIELDS;
            points.push(CausalAttributionPoint {
                coord_m: xi * (spec.low_band_arm_length_mm * 0.001),
                zone_label: if s < 3 {
                    "Coax Feed & CFRP Standoff Neck".into()
                } else if s < 7 {
                    "2.4 GHz High-Band Sleeve Trap".into()
                } else {
                    "900 MHz Low-Band Outer Tip & TPU Boot".into()
                },
                geometry_profile_m: spec.cfrp_standoff_mm * 0.001,
                primary_temperature_c: 25.0,
                forward_transport_kw: state_w[base] * 1.0e-3,
                return_or_loss_transport_kw: state_w[base + 2] * 1.0e-3,
                local_defect_birth: state_w[base + 2],
                downstream_survival_factor: (1.0 - 0.1 * xi).clamp(0.1, 1.0),
                surviving_exit_defect: state_w[base + 3],
            });
        }
        vec![CausalAttributionTrack {
            track_name: "Gemini Dipole Span s ∈ [0, L_900]: Standing-Wave Current, CFRP Eddy Loss & Azimuthal Gain".into(),
            coord_unit: "m".into(),
            geometry_label: "CFRP Standoff (m)".into(),
            defect_label: "Horizon Gain G_div".into(),
            defect_unit: "dBi".into(),
            points,
        }]
    }

    fn build_3d_fsm(&self, design_p: &[f64]) -> Option<String> {
        let theta = self.nominal_epistemic_vector();
        let spec = self.spec_from_params(design_p, &theta);
        Some(build_deadcat_drone_fsm(&spec))
    }
}
