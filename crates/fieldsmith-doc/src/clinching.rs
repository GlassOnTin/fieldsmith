//! Sheet-metal clinching (round press-joining / S-Lock) analytic & 3D parametric S-lock engine.
//!
//! References:
//! - Varis, J. (2003), "The suitability of round clinching tools for high strength structural
//!   steel", *Thin-Walled Structures*.
//! - Lee, C.-J., Kim, J.-Y., Lee, S.-K., Ko, D.-C., & Kim, B.-M. (2010), "Design of mechanical
//!   clinching tools for joining of aluminum alloy sheets", *Materials & Design* 31(4), 1854–1861.
//! - Mucha, J. (2011), "A study of quality parameters and behaviour of self-piercing riveted
//!   aluminium sheets with different joining conditions", *Strojniški vestnik*.
//! - Breda, A., Coppieters, S., & Debruyne, D. (2018), "Equivalent modelling strategy for a
//!   clinched joint using a simple calibration method", *Thin-Walled Structures* 113, 1–12.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use crate::materials;
use crate::model::{
    Consumer, Document, Face, Let, Load, LoadKind, Node, NodeKind, Op, PlasticityLaw, Region,
    Selector, Support, Transform, UncertaintySpec,
};
use crate::param::{self, ParamAddr};

fn default_t1_mm() -> f64 {
    1.5
}
fn default_t2_mm() -> f64 {
    1.5
}
fn default_punch_radius_mm() -> f64 {
    2.6
}
fn default_clearance_mm() -> f64 {
    0.75
}
fn default_punch_fillet_mm() -> f64 {
    0.30
}
fn default_bottom_x_mm() -> f64 {
    0.85
}
fn default_youngs_modulus_mpa() -> f64 {
    70_000.0
}
fn default_yield_strength_mpa() -> f64 {
    65.0
}
fn default_uts_mpa() -> f64 {
    240.0
}
fn default_hollomon_k_mpa() -> f64 {
    390.0
}
fn default_hardening_n() -> f64 {
    0.20
}
fn default_friction_mu() -> f64 {
    0.18
}
fn default_load_angle_deg() -> f64 {
    0.0
}
fn default_service_load_n() -> f64 {
    2_400.0
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Dominant failure mode along the clinching hat-curve `F_joint(X) = min(F_neck(X), F_pull(X))`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClinchingFailureMode {
    ButtonPullout,
    OptimalTransition,
    NeckFracture,
}

/// Sheet, tooling, constitutive, and service-load parameters for round press-joining (S-Lock).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClinchingSpec {
    /// Upper (punch-side) sheet thickness `t₁` (mm).
    #[serde(default = "default_t1_mm", alias = "t1Mm")]
    pub t1_mm: f64,
    /// Lower (die-side) sheet thickness `t₂` (mm).
    #[serde(default = "default_t2_mm", alias = "t2Mm")]
    pub t2_mm: f64,
    /// Punch radius `R_p` (mm).
    #[serde(default = "default_punch_radius_mm", alias = "punchRadiusMm")]
    pub punch_radius_mm: f64,
    /// Radial punch-to-die clearance `c = R_d - R_p` (mm).
    #[serde(default = "default_clearance_mm", alias = "clearanceMm")]
    pub clearance_mm: f64,
    /// Punch corner fillet radius `r_p` (mm) controlling 3D notch triaxiality `K_t`.
    #[serde(default = "default_punch_fillet_mm", alias = "punchFilletMm")]
    pub punch_fillet_mm: f64,
    /// Residual compressed bottom coin thickness `X` (mm) — primary shop-floor dial-in parameter.
    #[serde(default = "default_bottom_x_mm", alias = "bottomThicknessXMm")]
    pub bottom_thickness_x_mm: f64,
    /// Sheet metal elastic Young's modulus `E₀` (MPa).
    #[serde(default = "default_youngs_modulus_mpa", alias = "youngsModulusMpa")]
    pub youngs_modulus_mpa: f64,
    /// Initial yield strength `σ_y` (MPa).
    #[serde(default = "default_yield_strength_mpa", alias = "yieldStrengthMpa")]
    pub yield_strength_mpa: f64,
    /// Ultimate tensile strength `σ_UTS` (MPa).
    #[serde(default = "default_uts_mpa", alias = "utsMpa")]
    pub uts_mpa: f64,
    /// Hollomon strength coefficient `K` (MPa) in `σ = K ε^n`.
    #[serde(default = "default_hollomon_k_mpa", alias = "hollomonKMpa")]
    pub hollomon_k_mpa: f64,
    /// Hollomon / Ramberg–Osgood strain-hardening exponent `n` (dimensionless).
    #[serde(default = "default_hardening_n", alias = "hardeningExponentN")]
    pub hardening_exponent_n: f64,
    /// Interfacial sheet-to-sheet Coulomb friction coefficient `μ`.
    #[serde(default = "default_friction_mu", alias = "frictionMu")]
    pub friction_mu: f64,
    /// Service loading angle `θ_load` (deg): `0` = cross-tension, `45` = mixed peel/shear, `90` = lap-shear.
    #[serde(default = "default_load_angle_deg", alias = "loadAngleDeg")]
    pub load_angle_deg: f64,
    /// Applied service pull force `F_service` (N).
    #[serde(default = "default_service_load_n", alias = "serviceLoadN")]
    pub service_load_n: f64,
}

impl Default for ClinchingSpec {
    fn default() -> Self {
        Self {
            t1_mm: default_t1_mm(),
            t2_mm: default_t2_mm(),
            punch_radius_mm: default_punch_radius_mm(),
            clearance_mm: default_clearance_mm(),
            punch_fillet_mm: default_punch_fillet_mm(),
            bottom_thickness_x_mm: default_bottom_x_mm(),
            youngs_modulus_mpa: default_youngs_modulus_mpa(),
            yield_strength_mpa: default_yield_strength_mpa(),
            uts_mpa: default_uts_mpa(),
            hollomon_k_mpa: default_hollomon_k_mpa(),
            hardening_exponent_n: default_hardening_n(),
            friction_mu: default_friction_mu(),
            load_angle_deg: default_load_angle_deg(),
            service_load_n: default_service_load_n(),
        }
    }
}

/// Single-point evaluation of forming force, neck/undercut geometry, 1D Lee–Kim capacities,
/// and 3D Neuber–Hencky J₂ notch redistribution at bottom coin thickness `X`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClinchingPointEvaluation {
    pub x_mm: f64,
    pub coin_strain: f64,
    pub coin_flow_stress_mpa: f64,
    pub forming_force_kn: f64,
    pub neck_thickness_mm: f64,
    pub undercut_rigid_mm: f64,
    pub springback_loss_mm: f64,
    pub undercut_eff_mm: f64,
    pub lock_angle_deg: f64,
    pub neck_fracture_1d_n: f64,
    pub button_pullout_1d_n: f64,
    pub joint_strength_1d_n: f64,
    pub elastic_kt: f64,
    pub plastic_ksigma: f64,
    pub circumferential_arc_factor: f64,
    pub first_yield_3d_linear_n: f64,
    pub joint_strength_3d_nonlinear_n: f64,
    pub nominal_neck_stress_mpa: f64,
    pub linear_peak_notch_stress_mpa: f64,
    pub nonlinear_peak_neck_stress_mpa: f64,
    pub nonlinear_peak_plastic_strain_pct: f64,
    pub failure_mode: ClinchingFailureMode,
}

/// Complete clinching process-window analysis across `X ∈ [0.35, 1.60]` mm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClinchingSweepAnalysis {
    pub spec: ClinchingSpec,
    pub current: ClinchingPointEvaluation,
    /// Optimal bottom coin thickness `X*` (mm) maximizing `F_joint^1D(X)`.
    pub optimal_x_mm: f64,
    /// Peak 1D analytic joint strength at `X*` (N).
    pub peak_strength_1d_n: f64,
    /// Peak 3D nonlinear elasto-plastic joint strength across the sweep (N).
    pub peak_strength_3d_nonlinear_n: f64,
    /// Safe shop-floor process window `[X_min, X_max]` achieving `≥ 85%` of peak 1D strength (mm).
    pub window_range_mm: [f64; 2],
    /// 51-point hat-curve evaluation from `X = 0.35` mm to `X = 1.60` mm.
    pub curve: Vec<ClinchingPointEvaluation>,
}

/// Return `true` when `doc` represents a clinched S-lock joint or carries clinching `let` parameters.
pub fn is_clinching_document(doc: &Document) -> bool {
    let nm = doc.name.to_ascii_lowercase();
    if nm.contains("clinch") || nm.contains("a19") || nm.contains("t19") || nm.contains("s-lock") {
        return true;
    }
    doc.lets
        .iter()
        .any(|l| l.name == "c_die" || l.name == "mu_clinch")
        || doc.root.iter().any(|n| {
            n.id == "punch_pit"
                || n.id.ends_with("_punch_pit")
                || n.id == "die_ring_groove"
                || n.id.ends_with("_die_ring_groove")
        })
}

/// Extract [`ClinchingSpec`] from a document's `let` bindings, active material, and `solve` settings.
pub fn extract_clinching_spec_from_doc(doc: &Document) -> ClinchingSpec {
    let get_let = |names: &[&str], fallback: f64| -> f64 {
        for nm in names {
            if let Some(hit) = doc.lets.iter().find(|l| l.name == *nm) {
                if hit.value.is_finite() {
                    return hit.value;
                }
            }
        }
        fallback
    };

    let active_mat =
        materials::find(doc, &doc.material).or_else(|| doc.custom_materials.first().cloned());

    let e_mpa = active_mat
        .as_ref()
        .map(|m| m.e)
        .unwrap_or_else(default_youngs_modulus_mpa);
    let sy_mpa = active_mat
        .as_ref()
        .map(|m| m.yield_strength)
        .unwrap_or_else(|| get_let(&["sigma_y", "yield_mpa"], default_yield_strength_mpa()));

    let mat_k = active_mat.as_ref().and_then(|m| m.hardening_k);
    let mat_n = active_mat.as_ref().and_then(|m| m.hardening_n);

    let hollomon_k_mpa = (sy_mpa * 1.3).max(
        mat_k.unwrap_or_else(|| get_let(&["K_hollomon", "hollomon_k"], default_hollomon_k_mpa())),
    );
    let hardening_exponent_n = doc
        .solve
        .hardening_n
        .or(mat_n)
        .unwrap_or_else(|| get_let(&["hardening_n", "n_hard"], default_hardening_n()))
        .clamp(0.04, 0.45);

    // Considère necking criterion gives engineering UTS ≈ K · (n / e)^n when `sigma_uts` is not explicitly bound.
    let considere_uts = {
        let eps_u = hardening_exponent_n.max(0.05);
        hollomon_k_mpa * (eps_u / std::f64::consts::E).powf(eps_u)
    };
    let fallback_uts = if doc
        .lets
        .iter()
        .any(|l| l.name == "sigma_uts" || l.name == "uts_mpa")
    {
        default_uts_mpa()
    } else if active_mat.is_some() && (sy_mpa - default_yield_strength_mpa()).abs() > 1.0 {
        considere_uts.max(sy_mpa * 1.25)
    } else {
        default_uts_mpa()
    };
    let uts_mpa = (sy_mpa * 1.15).max(get_let(&["sigma_uts", "uts_mpa"], fallback_uts));

    ClinchingSpec {
        t1_mm: get_let(&["t1", "t_upper"], default_t1_mm()).max(0.4),
        t2_mm: get_let(&["t2", "t_lower"], default_t2_mm()).max(0.4),
        punch_radius_mm: get_let(&["Rp", "r_punch"], default_punch_radius_mm()).max(1.2),
        clearance_mm: get_let(&["c_die", "clearance"], default_clearance_mm()).max(0.3),
        punch_fillet_mm: get_let(&["r_fillet", "rp_fillet"], default_punch_fillet_mm()).max(0.08),
        bottom_thickness_x_mm: get_let(&["X", "bottom_x"], default_bottom_x_mm()).clamp(0.25, 2.2),
        youngs_modulus_mpa: e_mpa,
        yield_strength_mpa: sy_mpa,
        uts_mpa,
        hollomon_k_mpa,
        hardening_exponent_n,
        friction_mu: get_let(&["mu_clinch", "friction_mu"], default_friction_mu())
            .clamp(0.02, 0.55),
        load_angle_deg: get_let(&["load_angle_deg", "peel_angle"], default_load_angle_deg())
            .clamp(0.0, 90.0),
        service_load_n: get_let(&["F_pull", "service_load_n"], default_service_load_n()).max(100.0),
    }
}

/// Evaluate clinching mechanics at bottom coin thickness `x_override_mm` (or `spec.bottom_thickness_x_mm`).
pub fn evaluate_clinching_at_x(
    spec: &ClinchingSpec,
    x_override_mm: Option<f64>,
) -> ClinchingPointEvaluation {
    let t_total = (spec.t1_mm + spec.t2_mm).max(0.8);
    let x_mm = x_override_mm
        .unwrap_or(spec.bottom_thickness_x_mm)
        .clamp(0.25, t_total * 0.85);
    let rp = spec.punch_radius_mm.max(1.2);
    let c = spec.clearance_mm.max(0.3);
    let rd = rp + c;
    let n = spec.hardening_exponent_n.clamp(0.04, 0.45);
    let k_hol = spec.hollomon_k_mpa.max(spec.yield_strength_mpa * 1.1);

    // 1. Coin compression logarithmic strain & Hollomon mean flow stress
    let coin_strain = (t_total / x_mm).ln().max(0.08);
    let coin_flow_stress_mpa = (k_hol / (n + 1.0)) * coin_strain.powf(n);

    // 2. Breda–Coppieters–Avitzur axisymmetric upper-bound punch forming force (kN)
    let sqrt3 = 3.0_f64.sqrt();
    let m_shear = (sqrt3 * spec.friction_mu.max(0.0)).min(0.95);
    let slab_factor =
        1.0 + (2.0 * m_shear * rp) / (3.0 * sqrt3 * x_mm) + (2.0 / sqrt3) * (rd / rp).ln();
    let forming_force_kn = (PI * rp * rp * coin_flow_stress_mpa * slab_factor) * 1e-3;

    // 3. Isochoric neck thickness t_n(X) and interlock undercut t_u(X) (Varis 2003 / Mucha 2011)
    let x_ref = 0.52 * t_total;
    let stroke_depth = (x_ref - x_mm).max(0.0);
    let base_neck = c * (spec.t1_mm / t_total) + 0.14 * (spec.t1_mm / 1.5);
    let neck_thinning = 0.28 * (stroke_depth / (0.38 * t_total).max(0.4)).powf(1.28);
    let neck_thickness_mm = (base_neck - neck_thinning).clamp(0.12, c * 0.92);

    let radial_extrusion = 0.34 * (stroke_depth / (0.36 * t_total).max(0.4)).powf(1.35);
    let undercut_rigid_mm = (0.02 + radial_extrusion).clamp(0.015, 0.52);

    // Radial elastic springback relaxation after punch retraction
    let springback_loss_mm = (undercut_rigid_mm * 0.45).min(
        (coin_flow_stress_mpa / spec.youngs_modulus_mpa.max(10_000.0))
            * (rp + neck_thickness_mm)
            * 2.2,
    );
    let undercut_eff_mm = (undercut_rigid_mm - springback_loss_mm).max(0.008);

    // S-lock contact angle alpha_lock (deg)
    let lock_angle_deg = (8.0 + 48.0 * (undercut_eff_mm / c.max(0.3))).min(34.0);
    let alpha_rad = lock_angle_deg.to_radians();

    // 4. Lee–Kim (2010) 1D analytic failure loads (cross-tension & shear/peel interaction)
    let theta_rad = spec.load_angle_deg.clamp(0.0, 90.0).to_radians();
    let shear_interaction =
        1.0 / (theta_rad.cos().powi(2) + 3.0 * (0.45 * theta_rad.sin()).powi(2)).sqrt();
    let neck_ring_area_mm2 = 2.0 * PI * (rp + 0.5 * neck_thickness_mm) * neck_thickness_mm;
    let neck_fracture_1d_n = neck_ring_area_mm2 * spec.uts_mpa * shear_interaction;

    let mu = spec.friction_mu.clamp(0.02, 0.55);
    let friction_term = (alpha_rad.sin() + mu * alpha_rad.cos())
        / (alpha_rad.cos() - mu * alpha_rad.sin()).max(0.15);
    let plastic_unbending_term = (1.65 * undercut_rigid_mm) / (c + undercut_rigid_mm).max(0.25);
    let button_pullout_1d_n = 2.0
        * PI
        * rp
        * undercut_rigid_mm
        * coin_flow_stress_mpa
        * (friction_term + plastic_unbending_term);

    let joint_strength_1d_n = neck_fracture_1d_n.min(button_pullout_1d_n);

    // 5. Beyond-analytic 3D mechanics: notch triaxiality K_t, Neuber–Hencky K_sigma, & peel arc
    let elastic_kt = 1.0 + 1.45 * (neck_thickness_mm / spec.punch_fillet_mm.max(0.08)).sqrt();
    let plastic_ksigma = elastic_kt.powf((2.0 * n) / (n + 1.0));
    let circumferential_arc_factor = 1.0 - 0.34 * theta_rad.sin();

    let first_yield_3d_linear_n =
        (neck_ring_area_mm2 * spec.yield_strength_mpa * circumferential_arc_factor) / elastic_kt;

    let neck_fracture_3d_nl_n =
        (neck_fracture_1d_n * circumferential_arc_factor) / (0.72 * plastic_ksigma).max(1.0);
    let button_pullout_3d_nl_n = button_pullout_1d_n
        * (undercut_eff_mm / undercut_rigid_mm.max(1e-4))
        * circumferential_arc_factor;
    let joint_strength_3d_nonlinear_n = neck_fracture_3d_nl_n.min(button_pullout_3d_nl_n);

    // 6. Peak notch & redistributed neck stress + equivalent plastic strain at F_service
    let nominal_neck_stress_mpa =
        spec.service_load_n / (neck_ring_area_mm2 * circumferential_arc_factor).max(0.2);
    let linear_peak_notch_stress_mpa = nominal_neck_stress_mpa * elastic_kt;

    let mut nonlinear_peak_neck_stress_mpa = linear_peak_notch_stress_mpa;
    let mut nonlinear_peak_plastic_strain_pct = 0.0;
    if linear_peak_notch_stress_mpa > spec.yield_strength_mpa && spec.yield_strength_mpa > 1e-6 {
        let over_ratio = linear_peak_notch_stress_mpa / spec.yield_strength_mpa;
        nonlinear_peak_neck_stress_mpa =
            spec.yield_strength_mpa * over_ratio.powf((2.0 * n) / (n + 1.0));
        let e_eff = spec.youngs_modulus_mpa.max(1_000.0);
        let eps_y = spec.yield_strength_mpa / e_eff;
        let eps_total = eps_y * over_ratio.powf(2.0 / (n + 1.0));
        let eps_elastic = nonlinear_peak_neck_stress_mpa / e_eff;
        nonlinear_peak_plastic_strain_pct = ((eps_total - eps_elastic).max(0.0)) * 100.0;
    }

    let ratio_diff =
        (neck_fracture_1d_n - button_pullout_1d_n).abs() / joint_strength_1d_n.max(1.0);
    let failure_mode = if ratio_diff < 0.08 {
        ClinchingFailureMode::OptimalTransition
    } else if button_pullout_1d_n < neck_fracture_1d_n {
        ClinchingFailureMode::ButtonPullout
    } else {
        ClinchingFailureMode::NeckFracture
    };

    ClinchingPointEvaluation {
        x_mm,
        coin_strain,
        coin_flow_stress_mpa,
        forming_force_kn,
        neck_thickness_mm,
        undercut_rigid_mm,
        springback_loss_mm,
        undercut_eff_mm,
        lock_angle_deg,
        neck_fracture_1d_n,
        button_pullout_1d_n,
        joint_strength_1d_n,
        elastic_kt,
        plastic_ksigma,
        circumferential_arc_factor,
        first_yield_3d_linear_n,
        joint_strength_3d_nonlinear_n,
        nominal_neck_stress_mpa,
        linear_peak_notch_stress_mpa,
        nonlinear_peak_neck_stress_mpa,
        nonlinear_peak_plastic_strain_pct,
        failure_mode,
    }
}

/// Sweep `X ∈ [0.35, 1.60]` mm (`51` points), identify optimal `X*`, and report the `≥ 85%` process window.
pub fn analyze_clinching_process_window(spec: &ClinchingSpec) -> ClinchingSweepAnalysis {
    let x_min = 0.35_f64;
    let x_max = 1.60_f64;
    let steps = 51usize;
    let mut curve = Vec::with_capacity(steps);
    let mut best_idx = 0usize;
    let mut best_1d = -1.0_f64;
    let mut best_3d_nl = -1.0_f64;

    for i in 0..steps {
        let x = round3(x_min + ((x_max - x_min) * (i as f64)) / ((steps - 1) as f64));
        let pt = evaluate_clinching_at_x(spec, Some(x));
        if pt.joint_strength_1d_n > best_1d {
            best_1d = pt.joint_strength_1d_n;
            best_idx = i;
        }
        if pt.joint_strength_3d_nonlinear_n > best_3d_nl {
            best_3d_nl = pt.joint_strength_3d_nonlinear_n;
        }
        curve.push(pt);
    }

    let optimal_x_mm = curve.get(best_idx).map(|p| p.x_mm).unwrap_or(0.85);
    let threshold = 0.85 * best_1d;
    let valid_xs: Vec<f64> = curve
        .iter()
        .filter(|p| p.joint_strength_1d_n >= threshold)
        .map(|p| p.x_mm)
        .collect();
    let window_range_mm = match (valid_xs.first(), valid_xs.last()) {
        (Some(&lo), Some(&hi)) => [lo, hi],
        _ => [round3(optimal_x_mm - 0.10), round3(optimal_x_mm + 0.10)],
    };

    ClinchingSweepAnalysis {
        spec: spec.clone(),
        current: evaluate_clinching_at_x(spec, None),
        optimal_x_mm,
        peak_strength_1d_n: best_1d,
        peak_strength_3d_nonlinear_n: best_3d_nl,
        window_range_mm,
        curve,
    }
}

/// Request payload for [`DocCommand::ApplyClinching`]: dials in clinching process/load/J2 parameters
/// on an existing document or materializes a parametric 3D S-Lock clinched joint onto a host sheet or fresh coupon.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClinchingApplyRequest {
    /// Materialize a 3D S-lock clinched joint onto `host_id` (or as a standalone overlapping lap coupon if `host_id` is `None`).
    #[serde(default, alias = "insertJoint")]
    pub insert_joint: bool,
    /// Optional host sheet/flange node ID onto which to stamp the 3D S-lock button, die groove, and punch pit.
    #[serde(default, alias = "hostId")]
    pub host_id: Option<String>,
    /// Optional explicit `[x, y, z]` joint centre (mm) when stamping onto a host part.
    #[serde(default)]
    pub at: Option<[f64; 3]>,
    /// Optional ID prefix for inserted S-lock nodes when stamping onto a host part (default `"clinch"`).
    #[serde(default, alias = "idPrefix")]
    pub id_prefix: Option<String>,
    /// Optional sheet material ID (`"al5754_clinch"`, `"mat_dc04"`, etc.) to apply to the document.
    #[serde(default)]
    pub material: Option<String>,
    /// Automatically solve `analyze_clinching_process_window` and dial `X` to `optimal_x_mm` (`X*`).
    #[serde(default, alias = "dialOptimalX")]
    pub dial_optimal_x: bool,
    /// Explicit bottom coin thickness `X` (mm) override (ignored when `dial_optimal_x` is `true`).
    #[serde(default, alias = "bottomThicknessXMm")]
    pub bottom_thickness_x_mm: Option<f64>,
    #[serde(default, alias = "t1Mm")]
    pub t1_mm: Option<f64>,
    #[serde(default, alias = "t2Mm")]
    pub t2_mm: Option<f64>,
    #[serde(default, alias = "punchRadiusMm")]
    pub punch_radius_mm: Option<f64>,
    #[serde(default, alias = "clearanceMm")]
    pub clearance_mm: Option<f64>,
    #[serde(default, alias = "punchFilletMm")]
    pub punch_fillet_mm: Option<f64>,
    #[serde(default, alias = "frictionMu")]
    pub friction_mu: Option<f64>,
    /// Service loading angle `θ_load` (deg, `0..=90`). Also rotates `LoadKind::Force` vector(s) to `[|F| sin θ, 0, |F| cos θ]`.
    #[serde(default, alias = "loadAngleDeg")]
    pub load_angle_deg: Option<f64>,
    #[serde(default, alias = "serviceLoadN")]
    pub service_load_n: Option<f64>,
    /// Toggle 3D `solve plasticity=j2` secant elasto-plasticity (`Some(true)` enables J2 with `hardening_n`; `Some(false)` resets to linear Hookean).
    #[serde(default, alias = "j2Plasticity")]
    pub j2_plasticity: Option<bool>,
    /// Optional shop-floor micrometer / press-stop standard deviation `σ_X` (mm) to register an `UncertaintySpec` on `X`.
    #[serde(default, alias = "bottomXStdMm")]
    pub bottom_x_std_mm: Option<f64>,
}

/// Summary returned after applying [`ClinchingApplyRequest`] to a [`Document`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClinchingApplySummary {
    pub inserted_nodes: Vec<String>,
    pub updated_lets: Vec<String>,
    pub applied_x_mm: f64,
    pub optimal_x_mm: f64,
    pub j2_active: bool,
    pub analysis: ClinchingSweepAnalysis,
}

fn upsert_and_apply_let(doc: &mut Document, name: &str, val: f64, updated: &mut Vec<String>) {
    if let Some(slot) = doc.lets.iter().position(|l| l.name == name) {
        if let Ok(addr) = ParamAddr::parse(name) {
            let _ = param::apply(doc, &addr, val);
        }
        doc.lets[slot].value = val;
        doc.lets[slot].expr = format!("{val}");
    } else {
        doc.lets.push(Let {
            name: name.to_string(),
            expr: format!("{val}"),
            value: val,
        });
        if let Ok(addr) = ParamAddr::parse(name) {
            let _ = param::apply(doc, &addr, val);
        }
    }
    if !updated.iter().any(|n| n == name) {
        updated.push(name.to_string());
    }
}

fn make_node(id: impl Into<String>, op: Op, at: [f64; 3], kind: NodeKind) -> Node {
    Node {
        id: id.into(),
        op,
        blend: 0.0,
        suppressed: false,
        transform: Transform {
            translate: at,
            rotate: [0.0; 3],
        },
        material: None,
        color: None,
        kind,
    }
}

fn upsert_consumer(
    doc: &mut Document,
    target_id: &str,
    attr: &str,
    comp: Option<usize>,
    expr: &str,
) {
    if let Some(existing) = doc
        .consumed
        .iter_mut()
        .find(|c| c.target == target_id && c.attr == attr && c.comp == comp)
    {
        existing.expr = expr.to_string();
    } else {
        doc.consumed.push(Consumer {
            target: target_id.to_string(),
            attr: attr.to_string(),
            comp,
            expr: expr.to_string(),
        });
    }
}

fn find_node_bbox_in_root(nodes: &[Node], id: &str) -> Option<([f64; 3], [f64; 3])> {
    for n in nodes {
        let at = n.transform.translate;
        if n.id == id {
            return match &n.kind {
                NodeKind::Box { size, .. } => Some((at, *size)),
                NodeKind::Cylinder { radius, height, .. } => {
                    Some((at, [2.0 * radius, 2.0 * radius, *height]))
                }
                _ => Some((at, [26.0, 22.0, 3.0])),
            };
        }
        if let NodeKind::Group { children, .. } = &n.kind {
            if let Some((cat, csz)) = find_node_bbox_in_root(children, id) {
                return Some(([at[0] + cat[0], at[1] + cat[1], at[2] + cat[2]], csz));
            }
        }
    }
    None
}

/// Apply [`ClinchingApplyRequest`] to `doc`, updating parametric `let`s, rotating peel/shear
/// service loads, toggling `solve plasticity=j2`, optionally stamping 3D S-Lock CSG features,
/// and returning the updated [`ClinchingSweepAnalysis`].
pub fn apply_clinching_to_document(
    doc: &mut Document,
    req: &ClinchingApplyRequest,
) -> Result<ClinchingApplySummary, String> {
    if let Some(mat_id) = &req.material {
        let trimmed = mat_id.trim();
        if trimmed.is_empty() {
            return Err("apply_clinching: material cannot be empty".into());
        }
        if materials::find(doc, trimmed).is_none() {
            return Err(format!("apply_clinching: unknown material '{trimmed}'"));
        }
        doc.material = trimmed.to_string();
    }

    let mut inserted_nodes = Vec::new();
    let mut updated_lets = Vec::new();

    // Seed baseline clinching lets if we are inserting a joint onto a document that lacks them.
    if req.insert_joint {
        let cur = extract_clinching_spec_from_doc(doc);
        let t1 = req.t1_mm.unwrap_or(cur.t1_mm).max(0.4);
        let t2 = req.t2_mm.unwrap_or(cur.t2_mm).max(0.4);
        let rp = req.punch_radius_mm.unwrap_or(cur.punch_radius_mm).max(1.2);
        let c_die = req.clearance_mm.unwrap_or(cur.clearance_mm).max(0.3);
        let r_fil = req.punch_fillet_mm.unwrap_or(cur.punch_fillet_mm).max(0.08);
        let x0 = req
            .bottom_thickness_x_mm
            .unwrap_or(cur.bottom_thickness_x_mm)
            .clamp(0.25, (t1 + t2) * 0.85);
        let f_srv = req.service_load_n.unwrap_or(cur.service_load_n).max(100.0);
        let mu = req.friction_mu.unwrap_or(cur.friction_mu).clamp(0.02, 0.55);
        let ang = req
            .load_angle_deg
            .unwrap_or(cur.load_angle_deg)
            .clamp(0.0, 90.0);

        upsert_and_apply_let(doc, "X", x0, &mut updated_lets);
        upsert_and_apply_let(doc, "Rp", rp, &mut updated_lets);
        upsert_and_apply_let(doc, "c_die", c_die, &mut updated_lets);
        upsert_and_apply_let(doc, "r_fillet", r_fil, &mut updated_lets);
        upsert_and_apply_let(doc, "t1", t1, &mut updated_lets);
        upsert_and_apply_let(doc, "t2", t2, &mut updated_lets);
        upsert_and_apply_let(doc, "F_pull", f_srv, &mut updated_lets);
        upsert_and_apply_let(doc, "mu_clinch", mu, &mut updated_lets);
        upsert_and_apply_let(doc, "load_angle_deg", ang, &mut updated_lets);

        if let Some(host_id) = req.host_id.as_deref() {
            let (host_at, host_sz) = find_node_bbox_in_root(&doc.root, host_id)
                .ok_or_else(|| format!("apply_clinching: host node '{host_id}' not found"))?;
            let prefix = req
                .id_prefix
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("clinch");
            let j_at = req
                .at
                .unwrap_or([host_at[0], host_at[1], host_at[2] - 0.5 * host_sz[2]]);
            let t_stack = host_sz[2].max(t1 + t2);

            let btn_id = format!("{prefix}_die_button");
            let grv_id = format!("{prefix}_die_ring_groove");
            let boss_id = format!("{prefix}_upper_boss");
            let pit_id = format!("{prefix}_punch_pit");
            let grp_id = format!("{prefix}_slock");
            let pit_grp_id = format!("{prefix}_recess");

            let die_btn = make_node(
                btn_id.clone(),
                Op::Add,
                [0.0, 0.0, 1.0],
                NodeKind::Cylinder {
                    radius: rp + c_die + 1.8,
                    height: 2.0,
                    round: 0.0,
                },
            );
            let upper_boss = make_node(
                boss_id.clone(),
                Op::Add,
                [0.0, 0.0, t_stack + 0.6],
                NodeKind::Cylinder {
                    radius: rp + c_die + 1.4,
                    height: 2.2,
                    round: 0.0,
                },
            );
            let die_grv = make_node(
                grv_id.clone(),
                Op::Add,
                [0.0, 0.0, 0.25],
                NodeKind::Torus {
                    major: rp + c_die + 0.5,
                    minor: 0.55,
                },
            );
            let pit_h0 = (t_stack + 0.8 - x0).max(0.5);
            let punch_pit = make_node(
                pit_id.clone(),
                Op::Add,
                [0.0, 0.0, t_stack + 0.9],
                NodeKind::Cylinder {
                    radius: rp,
                    height: pit_h0,
                    round: 0.0,
                },
            );

            doc.root.push(make_node(
                grp_id.clone(),
                Op::Add,
                j_at,
                NodeKind::Group {
                    children: vec![die_btn, upper_boss],
                    modifiers: Vec::new(),
                },
            ));
            doc.root.push(make_node(
                pit_grp_id.clone(),
                Op::Subtract,
                j_at,
                NodeKind::Group {
                    children: vec![die_grv, punch_pit],
                    modifiers: Vec::new(),
                },
            ));

            let stack_plus = round3(t_stack + 0.8);
            upsert_consumer(doc, &btn_id, "radius", None, "Rp + c_die + 1.8");
            upsert_consumer(doc, &boss_id, "radius", None, "Rp + c_die + 1.4");
            upsert_consumer(doc, &grv_id, "major", None, "Rp + c_die + 0.5");
            upsert_consumer(doc, &pit_id, "radius", None, "Rp");
            upsert_consumer(doc, &pit_id, "height", None, &format!("{stack_plus} - X"));

            let reg_id = format!("{prefix}_pull_ring");
            if !doc.regions.iter().any(|r| r.id == reg_id) {
                doc.regions.push(Region {
                    id: reg_id.clone(),
                    selectors: vec![
                        Selector {
                            shape: boss_id.clone(),
                            face: Face::Top,
                            patch: None,
                        },
                        Selector {
                            shape: boss_id.clone(),
                            face: Face::Side,
                            patch: None,
                        },
                    ],
                });
            }
            let load_id = format!("{prefix}_pull");
            if !doc.loads.iter().any(|l| l.id == load_id) {
                let rad = ang.to_radians();
                doc.loads.push(Load {
                    id: load_id.clone(),
                    region: reg_id,
                    case: "Clinch Pull-Out".into(),
                    kind: LoadKind::Force {
                        vector: [round2(f_srv * rad.sin()), 0.0, round2(f_srv * rad.cos())],
                    },
                });
            }
            inserted_nodes.extend([grp_id, pit_grp_id, btn_id, grv_id, boss_id, pit_id]);
        } else {
            // Standalone overlapping clinched lap coupon (matching A19 / T19 canonical topology)
            if doc.name.trim().is_empty() || doc.name == "Untitled" {
                doc.name = "Sheet Metal Clinching S-Lock Coupon".into();
            }
            if req.material.is_none()
                && doc.custom_materials.is_empty()
                && (doc.material == "petg" || doc.material == "pla" || doc.material.is_empty())
            {
                doc.material = "al5754_clinch".into();
            }
            doc.root.retain(|n| {
                !matches!(
                    n.id.as_str(),
                    "lower_sheet"
                        | "upper_sheet"
                        | "die_button"
                        | "die_ring_groove"
                        | "upper_boss"
                        | "punch_pit"
                )
            });
            doc.root.extend([
                make_node(
                    "lower_sheet",
                    Op::Add,
                    [-2.0, 0.0, 0.5 * (t2 + 0.5)],
                    NodeKind::Box {
                        size: [26.0, 22.0, t2 + 0.5],
                        round: 0.0,
                    },
                ),
                make_node(
                    "upper_sheet",
                    Op::Add,
                    [2.0, 0.0, t2 + 0.5 * (t1 + 0.5)],
                    NodeKind::Box {
                        size: [26.0, 22.0, t1 + 0.5],
                        round: 0.0,
                    },
                ),
                make_node(
                    "die_button",
                    Op::Add,
                    [0.0, 0.0, 1.0],
                    NodeKind::Cylinder {
                        radius: rp + c_die + 1.8,
                        height: 2.0,
                        round: 0.0,
                    },
                ),
                make_node(
                    "die_ring_groove",
                    Op::Subtract,
                    [0.0, 0.0, 0.25],
                    NodeKind::Torus {
                        major: rp + c_die + 0.5,
                        minor: 0.55,
                    },
                ),
                make_node(
                    "upper_boss",
                    Op::Add,
                    [0.0, 0.0, t1 + t2 + 1.2],
                    NodeKind::Cylinder {
                        radius: rp + c_die + 1.4,
                        height: 2.8,
                        round: 0.0,
                    },
                ),
                make_node(
                    "punch_pit",
                    Op::Subtract,
                    [0.0, 0.0, t1 + t2 + 1.6],
                    NodeKind::Cylinder {
                        radius: rp,
                        height: (3.8 - x0).max(0.5),
                        round: 0.0,
                    },
                ),
            ]);
            upsert_consumer(doc, "lower_sheet", "size", Some(2), "t2 + 0.5");
            upsert_consumer(doc, "lower_sheet", "at", Some(2), "0.5 * (t2 + 0.5)");
            upsert_consumer(doc, "upper_sheet", "size", Some(2), "t1 + 0.5");
            upsert_consumer(doc, "upper_sheet", "at", Some(2), "t2 + 0.5 * (t1 + 0.5)");
            upsert_consumer(doc, "die_button", "radius", None, "Rp + c_die + 1.8");
            upsert_consumer(doc, "die_ring_groove", "major", None, "Rp + c_die + 0.5");
            upsert_consumer(doc, "upper_boss", "radius", None, "Rp + c_die + 1.4");
            upsert_consumer(doc, "upper_boss", "at", Some(2), "t1 + t2 + 1.2");
            upsert_consumer(doc, "punch_pit", "radius", None, "Rp");
            upsert_consumer(doc, "punch_pit", "height", None, "3.8 - X");
            upsert_consumer(doc, "punch_pit", "at", Some(2), "t1 + t2 + 1.6");

            if !doc.regions.iter().any(|r| r.id == "lower_clamp") {
                doc.regions.push(Region {
                    id: "lower_clamp".into(),
                    selectors: vec![
                        Selector {
                            shape: "lower_sheet".into(),
                            face: Face::Nx,
                            patch: None,
                        },
                        Selector {
                            shape: "lower_sheet".into(),
                            face: Face::Px,
                            patch: None,
                        },
                    ],
                });
            }
            if !doc.regions.iter().any(|r| r.id == "upper_pull_ring") {
                doc.regions.push(Region {
                    id: "upper_pull_ring".into(),
                    selectors: vec![
                        Selector {
                            shape: "upper_boss".into(),
                            face: Face::Top,
                            patch: None,
                        },
                        Selector {
                            shape: "upper_boss".into(),
                            face: Face::Side,
                            patch: None,
                        },
                    ],
                });
            }
            if !doc.supports.iter().any(|s| s.id == "die_fixture") {
                doc.supports.push(Support {
                    id: "die_fixture".into(),
                    region: "lower_clamp".into(),
                    fixed: [true, true, true],
                });
            }
            if !doc.loads.iter().any(|l| l.id == "cross_tension") {
                let rad = ang.to_radians();
                doc.loads.push(Load {
                    id: "cross_tension".into(),
                    region: "upper_pull_ring".into(),
                    case: "Cross-Tension Pull-Out".into(),
                    kind: LoadKind::Force {
                        vector: [round2(f_srv * rad.sin()), 0.0, round2(f_srv * rad.cos())],
                    },
                });
            }
            inserted_nodes.extend([
                "lower_sheet".into(),
                "upper_sheet".into(),
                "die_button".into(),
                "die_ring_groove".into(),
                "upper_boss".into(),
                "punch_pit".into(),
            ]);
        }
    }

    // Apply any explicit tooling / sheet / friction / service load overrides first
    if let Some(v) = req.t1_mm {
        upsert_and_apply_let(doc, "t1", v.max(0.4), &mut updated_lets);
    }
    if let Some(v) = req.t2_mm {
        upsert_and_apply_let(doc, "t2", v.max(0.4), &mut updated_lets);
    }
    if let Some(v) = req.punch_radius_mm {
        upsert_and_apply_let(doc, "Rp", v.max(1.2), &mut updated_lets);
    }
    if let Some(v) = req.clearance_mm {
        upsert_and_apply_let(doc, "c_die", v.max(0.3), &mut updated_lets);
    }
    if let Some(v) = req.punch_fillet_mm {
        upsert_and_apply_let(doc, "r_fillet", v.max(0.08), &mut updated_lets);
    }
    if let Some(v) = req.friction_mu {
        upsert_and_apply_let(doc, "mu_clinch", v.clamp(0.02, 0.55), &mut updated_lets);
    }
    if let Some(v) = req.service_load_n {
        upsert_and_apply_let(doc, "F_pull", v.max(100.0), &mut updated_lets);
    }

    // Toggle J2 plasticity before extracting spec so `hardening_n` is consistent
    let enable_j2 = req.j2_plasticity.or_else(|| {
        if req.insert_joint && doc.solve.plasticity.is_none() {
            Some(true)
        } else {
            None
        }
    });
    if let Some(j2) = enable_j2 {
        if j2 {
            let cur_spec = extract_clinching_spec_from_doc(doc);
            doc.solve.plasticity = Some(PlasticityLaw::J2);
            doc.solve.hardening_n = Some(cur_spec.hardening_exponent_n.clamp(0.01, 0.80));
        } else {
            doc.solve.plasticity = None;
            doc.solve.hardening_n = None;
        }
        doc.lets.retain(|l| l.name != "nonlinear_plastic");
    }

    if let Some(ang) = req.load_angle_deg {
        let clamped = ang.clamp(0.0, 90.0);
        upsert_and_apply_let(doc, "load_angle_deg", clamped, &mut updated_lets);
        let rad = clamped.to_radians();
        let fallback_mag = req
            .service_load_n
            .unwrap_or_else(|| extract_clinching_spec_from_doc(doc).service_load_n);
        for ld in &mut doc.loads {
            if let LoadKind::Force { vector } = &mut ld.kind {
                let mag =
                    (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt();
                let m = if mag > 1e-6 { mag } else { fallback_mag };
                *vector = [round2(m * rad.sin()), 0.0, round2(m * rad.cos())];
            }
        }
        // Remove any consumer pinning a single Z component when rotating into 3D peel/shear
        if clamped > 1e-6 {
            doc.consumed
                .retain(|c| !(c.attr == "force" && doc.loads.iter().any(|l| l.id == c.target)));
        }
    }

    // Dial X either to optimal X* or explicit `bottom_thickness_x_mm`
    if req.dial_optimal_x {
        let pre_spec = extract_clinching_spec_from_doc(doc);
        let pre_win = analyze_clinching_process_window(&pre_spec);
        upsert_and_apply_let(doc, "X", pre_win.optimal_x_mm, &mut updated_lets);
    } else if let Some(x_val) = req.bottom_thickness_x_mm {
        let cur = extract_clinching_spec_from_doc(doc);
        let x_clamped = x_val.clamp(0.25, (cur.t1_mm + cur.t2_mm) * 0.85);
        upsert_and_apply_let(doc, "X", x_clamped, &mut updated_lets);
    }

    if let Some(std_mm) = req.bottom_x_std_mm {
        if std_mm > 0.0 && std_mm.is_finite() {
            if let Some(existing) = doc.uncertainties.iter_mut().find(|u| u.param == "X") {
                existing.std = Some(std_mm);
                existing.cv = None;
            } else {
                doc.uncertainties.push(UncertaintySpec {
                    label: "Clinch bottom coin thickness scatter".into(),
                    param: "X".into(),
                    std: Some(std_mm),
                    cv: None,
                    unit: "mm".into(),
                });
            }
        }
    }

    let final_spec = extract_clinching_spec_from_doc(doc);
    let analysis = analyze_clinching_process_window(&final_spec);
    let j2_active = doc.solve.plasticity == Some(PlasticityLaw::J2);

    Ok(ClinchingApplySummary {
        inserted_nodes,
        updated_lets,
        applied_x_mm: final_spec.bottom_thickness_x_mm,
        optimal_x_mm: analysis.optimal_x_mm,
        j2_active,
        analysis,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsm;

    #[test]
    fn a19_default_clinching_matches_closed_form_lee_kim_and_breda_benchmarks() {
        let spec = ClinchingSpec::default();
        let win = analyze_clinching_process_window(&spec);
        let cur = &win.current;

        // 1. Breda–Avitzur forming force at X = 0.85 mm, R_p = 2.6 mm: exact closed-form slab identity (~12.00 kN)
        let eps_coin = (3.0_f64 / 0.85).ln();
        let sigma_bar = (390.0 / 1.20) * eps_coin.powf(0.20);
        let m_sh = (3.0_f64.sqrt() * 0.18).min(0.95);
        let slab = 1.0
            + (2.0 * m_sh * 2.6) / (3.0 * 3.0_f64.sqrt() * 0.85)
            + (2.0 / 3.0_f64.sqrt()) * (3.35_f64 / 2.6).ln();
        let expected_f_punch_kn = PI * 2.6 * 2.6 * sigma_bar * slab * 1e-3;
        assert!(
            (cur.forming_force_kn - expected_f_punch_kn).abs() < 1e-9,
            "forming_force_kn = {} vs {}",
            cur.forming_force_kn,
            expected_f_punch_kn
        );
        // 2. Varis–Mucha neck & post-springback undercut at X = 0.85 mm
        assert!(
            (cur.neck_thickness_mm - 0.3623).abs() < 0.005,
            "neck_thickness_mm = {}",
            cur.neck_thickness_mm
        );
        assert!(
            cur.undercut_eff_mm > 0.14 && cur.undercut_eff_mm < cur.undercut_rigid_mm,
            "undercut_eff_mm = {} vs rigid {}",
            cur.undercut_eff_mm,
            cur.undercut_rigid_mm
        );
        // 3. Optimal coin thickness X* lies inside [0.70, 0.90] mm
        assert!(
            (0.70..=0.90).contains(&win.optimal_x_mm),
            "optimal_x_mm = {}",
            win.optimal_x_mm
        );
        assert!(
            win.window_range_mm[0] < win.optimal_x_mm && win.window_range_mm[1] > win.optimal_x_mm
        );

        // 4. Neuber–Hencky J2 stress redistribution: K_sigma = K_t^(2n/(n+1)) < K_t
        let expected_ksigma = cur.elastic_kt.powf((2.0 * 0.20) / 1.20);
        assert!((cur.plastic_ksigma - expected_ksigma).abs() < 1e-12);
        assert!(cur.plastic_ksigma < cur.elastic_kt);

        // 5. Over-clinched (X = 0.50 mm) fails by NeckFracture; under-clinched (X = 1.25 mm) by ButtonPullout
        let over = evaluate_clinching_at_x(&spec, Some(0.50));
        let under = evaluate_clinching_at_x(&spec, Some(1.25));
        assert_eq!(over.failure_mode, ClinchingFailureMode::NeckFracture);
        assert_eq!(under.failure_mode, ClinchingFailureMode::ButtonPullout);
    }

    #[test]
    fn apply_clinching_inserts_coupon_dials_optimal_x_and_rotates_peel_load() {
        let mut doc = Document::default();
        let sum = apply_clinching_to_document(
            &mut doc,
            &ClinchingApplyRequest {
                insert_joint: true,
                material: Some("mat_dc04".into()),
                bottom_thickness_x_mm: Some(1.25),
                dial_optimal_x: false,
                bottom_x_std_mm: Some(0.05),
                ..Default::default()
            },
        )
        .expect("insert DC04 clinch coupon");
        assert_eq!(sum.inserted_nodes.len(), 6);
        assert!(sum.j2_active);
        assert_eq!(doc.uncertainties.len(), 1);

        // Now dial optimal X* and rotate to 45° mixed peel/shear
        let dialed = apply_clinching_to_document(
            &mut doc,
            &ClinchingApplyRequest {
                dial_optimal_x: true,
                load_angle_deg: Some(45.0),
                ..Default::default()
            },
        )
        .expect("dial optimal X*");
        assert!((dialed.applied_x_mm - dialed.optimal_x_mm).abs() < 1e-9);

        let printed = fsm::print(&doc);
        let reparsed = fsm::parse(&printed).expect("reparse clinched .fsm");
        assert_eq!(reparsed.solve.plasticity, Some(PlasticityLaw::J2));
        assert_eq!(reparsed.uncertainties.len(), 1);
        if let LoadKind::Force { vector } = &reparsed.loads[0].kind {
            assert!(
                (vector[0] - vector[2]).abs() < 0.05 && vector[0] > 1000.0,
                "expected 45° rotated load vector, got {vector:?}"
            );
        } else {
            panic!("expected Force load");
        }
    }
}
