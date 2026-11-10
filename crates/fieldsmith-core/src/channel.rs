//! General SDF Cross-Section & 1D Axial Channel Flow / Convective-Film Operator.
//!
//! Extracts exact 2D cross-sectional area `A(s)`, wetted perimeter `P_w(s)`, and hydraulic
//! diameter `D_h(s) = 4 A(s) / P_w(s)` directly from any `.fsm` implicit signed-distance field
//! along a chosen axis (`x`, `y`, or `z`) via root-polished marching squares
//! ([`fieldsmith_mesh::slice::slice_field`]), marches 1D steady internal flow
//! (Darcy–Weisbach / Haaland friction + Bernoulli area-change pressure + laminar/Gnielinski
//! Nusselt convection + bulk enthalpy balance), and optionally binds the resulting convective
//! film and fluid pressure onto a named `.fsm` region for 3D P2 thermo-structural FEA.

use fieldsmith_doc::{materials, Convection, Document, Load, LoadKind};
use fieldsmith_field::expr;
use fieldsmith_mesh::slice::{polygon_signed_area, slice_field};
use serde::{Deserialize, Serialize};

/// Principal coordinate axis along which cross-sections are marched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelAxis {
    X,
    Y,
    #[default]
    Z,
}

impl ChannelAxis {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "x" => Ok(Self::X),
            "y" => Ok(Self::Y),
            "z" | "" => Ok(Self::Z),
            other => Err(format!("invalid channel axis '{other}' (expected x, y, or z)")),
        }
    }
}

/// Which closed contours in each slice represent the flow passage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PassageMode {
    /// Prefer interior void loops (CW holes inside a solid housing/manifold/TPMS); fall back to
    /// solid outer loops if a slice has no enclosed holes (e.g. when `node_id` names a fluid core).
    #[default]
    Auto,
    /// Measure only enclosed interior holes (`signed_area < 0`) inside the solid body.
    InteriorVoid,
    /// Measure positive solid cross-sections (`signed_area > 0`), e.g. an explicitly modelled fluid plug.
    SolidCore,
}

impl PassageMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Ok(Self::Auto),
            "void" | "interior" | "interior_void" | "hole" => Ok(Self::InteriorVoid),
            "solid" | "core" | "solid_core" => Ok(Self::SolidCore),
            other => Err(format!(
                "invalid passage mode '{other}' (expected auto, interior_void, or solid_core)"
            )),
        }
    }
}

/// Input specification for `analyze_sdf_channel`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelFlowSpec {
    #[serde(default)]
    pub axis: ChannelAxis,
    #[serde(default)]
    pub passage_mode: PassageMode,
    /// Optional shape node id to slice directly instead of the full document boolean tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    /// Number of axial stations along `[s_min, s_max]` (default 24, clamped to `4..=200`).
    #[serde(default = "default_stations")]
    pub stations: usize,
    /// Optional `[s_start, s_end]` axial bounds in mm (defaults to 2%–98% of body span along `axis`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_mm: Option<[f64; 2]>,
    /// Transverse marching-squares grid spacing `h` in mm (default auto ~ 1/64 of transverse span).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slice_h_mm: Option<f64>,
    /// Mass flow rate `ṁ` in kg/s.
    #[serde(default = "default_mass_flow")]
    pub mass_flow_kg_s: f64,
    /// Fluid density `ρ` in kg/m³ (default water: `997.0`).
    #[serde(default = "default_density")]
    pub density_kg_m3: f64,
    /// Dynamic viscosity `μ` in Pa·s (default water at 25 °C: `8.9e-4`).
    #[serde(default = "default_viscosity")]
    pub viscosity_pa_s: f64,
    /// Fluid thermal conductivity `k_f` in W/(m·K) (default water: `0.60`).
    #[serde(default = "default_conductivity")]
    pub conductivity_w_m_k: f64,
    /// Specific heat capacity `c_p` in J/(kg·K) (default water: `4182.0`).
    #[serde(default = "default_cp")]
    pub cp_j_kg_k: f64,
    /// Surface sand-grain roughness `ε` in mm (default `0.005` mm).
    #[serde(default = "default_roughness")]
    pub roughness_mm: f64,
    /// Static pressure at the first station `p_in` in MPa (default `0.5` MPa).
    #[serde(default = "default_inlet_p")]
    pub inlet_pressure_mpa: f64,
    /// Bulk fluid inlet temperature `T_in` in °C (default `20.0` °C).
    #[serde(default = "default_inlet_t")]
    pub inlet_temp_c: f64,
    /// Effective wall temperature `T_wall` in °C driving 1D enthalpy exchange (default `120.0` °C).
    #[serde(default = "default_wall_t")]
    pub wall_temp_c: f64,
}

fn default_stations() -> usize {
    24
}
fn default_mass_flow() -> f64 {
    0.05
}
fn default_density() -> f64 {
    997.0
}
fn default_viscosity() -> f64 {
    8.9e-4
}
fn default_conductivity() -> f64 {
    0.60
}
fn default_cp() -> f64 {
    4182.0
}
fn default_roughness() -> f64 {
    0.005
}
fn default_inlet_p() -> f64 {
    0.50
}
fn default_inlet_t() -> f64 {
    20.0
}
fn default_wall_t() -> f64 {
    120.0
}

impl Default for ChannelFlowSpec {
    fn default() -> Self {
        Self {
            axis: ChannelAxis::Z,
            passage_mode: PassageMode::Auto,
            node_id: None,
            stations: default_stations(),
            span_mm: None,
            slice_h_mm: None,
            mass_flow_kg_s: default_mass_flow(),
            density_kg_m3: default_density(),
            viscosity_pa_s: default_viscosity(),
            conductivity_w_m_k: default_conductivity(),
            cp_j_kg_k: default_cp(),
            roughness_mm: default_roughness(),
            inlet_pressure_mpa: default_inlet_p(),
            inlet_temp_c: default_inlet_t(),
            wall_temp_c: default_wall_t(),
        }
    }
}

/// Extracted geometry and solved 1D thermo-fluid state at one axial coordinate `s`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelStation {
    pub s_mm: f64,
    pub passage_count: usize,
    pub area_mm2: f64,
    pub wetted_perimeter_mm: f64,
    pub hydraulic_diameter_mm: f64,
    pub velocity_m_s: f64,
    pub reynolds: f64,
    pub friction_factor: f64,
    pub nusselt: f64,
    /// Physical convective heat-transfer coefficient `h_conv` in W/(m²·K).
    pub h_conv_w_m2_k: f64,
    /// Normalized `.fsm` `convection h=` parameter `(h_conv / k_solid) * 1e-3` (1/mm).
    pub h_fsm_per_mm: f64,
    pub static_pressure_mpa: f64,
    pub bulk_temp_c: f64,
}

/// Complete 1D SDF channel profile summary + station table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelProfileReport {
    pub axis: ChannelAxis,
    pub slice_h_mm: f64,
    pub length_mm: f64,
    pub min_area_mm2: f64,
    pub max_area_mm2: f64,
    pub mean_hydraulic_diameter_mm: f64,
    pub viscous_pressure_drop_mpa: f64,
    pub outlet_pressure_mpa: f64,
    pub outlet_temp_c: f64,
    pub mean_h_conv_w_m2_k: f64,
    pub mean_h_fsm_per_mm: f64,
    pub mean_pressure_mpa: f64,
    pub mean_bulk_temp_c: f64,
    pub total_heat_transfer_w: f64,
    pub stations: Vec<ChannelStation>,
}

fn polygon_perimeter(poly: &[[f64; 2]]) -> f64 {
    let n = poly.len();
    if n < 2 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        sum += (dx * dx + dy * dy).sqrt();
    }
    sum
}

/// Haaland explicit Darcy–Weisbach friction factor with smooth laminar-to-turbulent blend.
fn darcy_friction_factor(re: f64, rel_roughness: f64) -> f64 {
    if re <= 1e-6 {
        return 0.0;
    }
    let f_lam = 64.0 / re.max(1.0);
    if re <= 2100.0 {
        return f_lam;
    }
    let re_turb = re.max(2300.0);
    let term = (rel_roughness / 3.7).powf(1.11) + 6.9 / re_turb;
    let inv_sqrt_f = -1.8 * term.max(1e-16).log10();
    let f_turb = (1.0 / inv_sqrt_f.max(1e-6)).powi(2);
    if re < 3500.0 {
        let w = ((re - 2100.0) / 1400.0).clamp(0.0, 1.0);
        (1.0 - w) * f_lam + w * f_turb
    } else {
        f_turb
    }
}

/// Internal pipe/duct Nusselt number (3.66 laminar constant-wall-T, Gnielinski turbulent).
fn internal_nusselt(re: f64, pr: f64, f_darcy: f64) -> f64 {
    let nu_lam = 3.66;
    if re <= 2300.0 {
        return nu_lam;
    }
    let f8 = 0.125 * f_darcy;
    let denom = 1.0 + 12.7 * f8.sqrt() * (pr.max(1e-4).powf(2.0 / 3.0) - 1.0);
    let nu_gn = (f8 * (re - 1000.0) * pr) / denom.max(1e-6);
    if re < 3500.0 {
        let w = ((re - 2300.0) / 1200.0).clamp(0.0, 1.0);
        (1.0 - w) * nu_lam + w * nu_gn.max(nu_lam)
    } else {
        nu_gn.max(nu_lam)
    }
}

/// Slice a `.fsm` document's SDF perpendicular to `spec.axis` and solve the 1D axial flow and
/// convective heat-transfer profile across the extracted cross-sections.
pub fn analyze_sdf_channel(
    doc: &Document,
    spec: &ChannelFlowSpec,
) -> Result<ChannelProfileReport, String> {
    if !spec.mass_flow_kg_s.is_finite() || spec.mass_flow_kg_s < 0.0 {
        return Err("mass_flow_kg_s must be finite and >= 0".into());
    }
    if !spec.density_kg_m3.is_finite() || spec.density_kg_m3 <= 0.0 {
        return Err("density_kg_m3 must be finite and > 0".into());
    }
    if !spec.viscosity_pa_s.is_finite() || spec.viscosity_pa_s <= 0.0 {
        return Err("viscosity_pa_s must be finite and > 0".into());
    }
    if !spec.conductivity_w_m_k.is_finite() || spec.conductivity_w_m_k <= 0.0 {
        return Err("conductivity_w_m_k must be finite and > 0".into());
    }
    if !spec.cp_j_kg_k.is_finite() || spec.cp_j_kg_k <= 0.0 {
        return Err("cp_j_kg_k must be finite and > 0".into());
    }

    let compiled = if let Some(ref nid) = spec.node_id {
        let node = doc
            .find(nid)
            .ok_or_else(|| format!("channel node_id '{nid}' not found in document"))?;
        expr::compile_node(doc, node)
    } else {
        expr::compile_body(doc)
    };
    let b = expr::bounds(&compiled);
    if b.is_empty() {
        return Err("cannot extract channel profile: compiled SDF bounds are empty".into());
    }

    // Permute coordinates so slice_field's Z axis aligns with `spec.axis`.
    let (u_ax, v_ax, s_ax) = match spec.axis {
        ChannelAxis::X => (1usize, 2usize, 0usize),
        ChannelAxis::Y => (2usize, 0usize, 1usize),
        ChannelAxis::Z => (0usize, 1usize, 2usize),
    };
    let lo_perm = [b.lo[u_ax], b.lo[v_ax], b.lo[s_ax]];
    let hi_perm = [b.hi[u_ax], b.hi[v_ax], b.hi[s_ax]];

    let span_u = (hi_perm[0] - lo_perm[0]).max(1e-3);
    let span_v = (hi_perm[1] - lo_perm[1]).max(1e-3);
    let slice_h = spec
        .slice_h_mm
        .unwrap_or_else(|| (span_u.max(span_v) / 72.0).clamp(0.05, 2.0))
        .max(1e-3);

    let (s0, s1) = if let Some([a, c]) = spec.span_mm {
        if !(a.is_finite() && c.is_finite() && c > a) {
            return Err("span_mm must satisfy s_end > s_start".into());
        }
        (a, c)
    } else {
        let full = hi_perm[2] - lo_perm[2];
        if full <= 1e-6 {
            return Err("model has zero extent along the requested channel axis".into());
        }
        // Inset slightly by 2% so open pipe endcaps do not land on singular z=lo/hi planes.
        (lo_perm[2] + 0.02 * full, hi_perm[2] - 0.02 * full)
    };

    let n_stations = spec.stations.clamp(4, 200);
    let solid_k_w_m_k = materials::find(doc, &doc.material)
        .and_then(|m| m.k)
        .unwrap_or(150.0)
        .max(1e-6);
    let pr = spec.cp_j_kg_k * spec.viscosity_pa_s / spec.conductivity_w_m_k;

    let map_pt = |uvs: [f64; 3]| -> [f64; 3] {
        let mut world = [0.0; 3];
        world[u_ax] = uvs[0];
        world[v_ax] = uvs[1];
        world[s_ax] = uvs[2];
        world
    };

    let mut geom_rows: Vec<(f64, usize, f64, f64)> = Vec::with_capacity(n_stations);
    for k in 0..n_stations {
        let frac = k as f64 / (n_stations - 1) as f64;
        let s = s0 + frac * (s1 - s0);
        let layer = slice_field(|uvs| expr::eval(&compiled, map_pt(uvs)).v, lo_perm, hi_perm, s, slice_h);

        let mut void_area = 0.0f64;
        let mut void_perim = 0.0f64;
        let mut void_count = 0usize;
        let mut solid_area = 0.0f64;
        let mut solid_perim = 0.0f64;
        let mut solid_count = 0usize;

        for poly in &layer.polygons {
            let sa = polygon_signed_area(poly);
            let per = polygon_perimeter(poly);
            if sa < -1e-9 {
                void_area += -sa;
                void_perim += per;
                void_count += 1;
            } else if sa > 1e-9 {
                solid_area += sa;
                solid_perim += per;
                solid_count += 1;
            }
        }

        let use_void = match spec.passage_mode {
            PassageMode::InteriorVoid => true,
            PassageMode::SolidCore => false,
            PassageMode::Auto => spec.node_id.is_none() && void_count > 0,
        };
        let (cnt, area, perim) = if use_void {
            (void_count, void_area, void_perim)
        } else {
            (solid_count, solid_area, solid_perim)
        };

        if area > 1e-6 && perim > 1e-6 {
            geom_rows.push((s, cnt, area, perim));
        }
    }

    if geom_rows.len() < 2 {
        return Err(
            "fewer than 2 valid flow cross-sections found along axis (check axis or passage_mode)"
                .into(),
        );
    }

    let mut stations = Vec::with_capacity(geom_rows.len());
    let mut p_visc_loss_pa = 0.0f64;
    let mut t_bulk = spec.inlet_temp_c;
    let mut u_inlet_m_s = 0.0f64;

    for (idx, &(s_mm, cnt, area_mm2, perim_mm)) in geom_rows.iter().enumerate() {
        let area_m2 = area_mm2 * 1e-6;
        let perim_m = perim_mm * 1e-3;
        let dh_mm = 4.0 * area_mm2 / perim_mm;
        let dh_m = dh_mm * 1e-3;
        let u_m_s = spec.mass_flow_kg_s / (spec.density_kg_m3 * area_m2);
        if idx == 0 {
            u_inlet_m_s = u_m_s;
        }
        let re = spec.density_kg_m3 * u_m_s * dh_m / spec.viscosity_pa_s;
        let rel_rough = (spec.roughness_mm / dh_mm.max(1e-6)).max(0.0);
        let f_d = darcy_friction_factor(re, rel_rough);
        let nu = internal_nusselt(re, pr, f_d);
        let h_conv = nu * spec.conductivity_w_m_k / dh_m.max(1e-9);
        let h_fsm = (h_conv / solid_k_w_m_k) * 1e-3;

        if idx > 0 {
            let ds_m = (s_mm - geom_rows[idx - 1].0) * 1e-3;
            let prev: &ChannelStation = &stations[idx - 1];
            let dh_mid_m = 0.5 * (prev.hydraulic_diameter_mm + dh_mm) * 1e-3;
            let u_mid = 0.5 * (prev.velocity_m_s + u_m_s);
            let f_mid = 0.5 * (prev.friction_factor + f_d);
            p_visc_loss_pa += f_mid * (ds_m / dh_mid_m.max(1e-9)) * 0.5 * spec.density_kg_m3 * u_mid * u_mid;

            if spec.mass_flow_kg_s > 1e-12 {
                let h_mid = 0.5 * (prev.h_conv_w_m2_k + h_conv);
                let perim_mid_m = 0.5 * (prev.wetted_perimeter_mm * 1e-3 + perim_m);
                let ntu_step = (h_mid * perim_mid_m * ds_m) / (spec.mass_flow_kg_s * spec.cp_j_kg_k);
                // Exact exponential relaxation over step ds so large NTU steps never overshoot T_wall:
                t_bulk = spec.wall_temp_c + (t_bulk - spec.wall_temp_c) * (-ntu_step).exp();
            }
        }

        let dyn_change_pa = 0.5 * spec.density_kg_m3 * (u_inlet_m_s * u_inlet_m_s - u_m_s * u_m_s);
        let p_static_mpa = spec.inlet_pressure_mpa + (dyn_change_pa - p_visc_loss_pa) * 1e-6;

        stations.push(ChannelStation {
            s_mm,
            passage_count: cnt,
            area_mm2,
            wetted_perimeter_mm: perim_mm,
            hydraulic_diameter_mm: dh_mm,
            velocity_m_s: u_m_s,
            reynolds: re,
            friction_factor: f_d,
            nusselt: nu,
            h_conv_w_m2_k: h_conv,
            h_fsm_per_mm: h_fsm,
            static_pressure_mpa: p_static_mpa,
            bulk_temp_c: t_bulk,
        });
    }

    let n = stations.len() as f64;
    let length_mm = stations.last().unwrap().s_mm - stations.first().unwrap().s_mm;
    let min_area_mm2 = stations.iter().map(|s| s.area_mm2).fold(f64::INFINITY, f64::min);
    let max_area_mm2 = stations.iter().map(|s| s.area_mm2).fold(0.0, f64::max);
    let mean_hydraulic_diameter_mm = stations.iter().map(|s| s.hydraulic_diameter_mm).sum::<f64>() / n;
    let mean_h_conv_w_m2_k = stations.iter().map(|s| s.h_conv_w_m2_k).sum::<f64>() / n;
    let mean_h_fsm_per_mm = stations.iter().map(|s| s.h_fsm_per_mm).sum::<f64>() / n;
    let mean_pressure_mpa = stations.iter().map(|s| s.static_pressure_mpa).sum::<f64>() / n;
    let mean_bulk_temp_c = stations.iter().map(|s| s.bulk_temp_c).sum::<f64>() / n;
    let outlet_pressure_mpa = stations.last().unwrap().static_pressure_mpa;
    let outlet_temp_c = stations.last().unwrap().bulk_temp_c;
    let total_heat_transfer_w = spec.mass_flow_kg_s * spec.cp_j_kg_k * (outlet_temp_c - spec.inlet_temp_c);

    Ok(ChannelProfileReport {
        axis: spec.axis,
        slice_h_mm: slice_h,
        length_mm,
        min_area_mm2,
        max_area_mm2,
        mean_hydraulic_diameter_mm,
        viscous_pressure_drop_mpa: p_visc_loss_pa * 1e-6,
        outlet_pressure_mpa,
        outlet_temp_c,
        mean_h_conv_w_m2_k,
        mean_h_fsm_per_mm,
        mean_pressure_mpa,
        mean_bulk_temp_c,
        total_heat_transfer_w,
        stations,
    })
}

/// Bind a solved [`ChannelProfileReport`]'s mean convective film (`h`, `T_∞`) and internal fluid
/// pressure onto `region` in `doc` so 3D P2 thermo-structural FEA solves with geometry-derived BCs.
pub fn bind_channel_bcs_to_region(
    doc: &mut Document,
    report: &ChannelProfileReport,
    region: &str,
    thermal_case: &str,
    pressure_case: Option<&str>,
) -> Result<(), String> {
    if !doc.regions.iter().any(|r| r.id == region) {
        return Err(format!("unknown region '{region}' in document"));
    }
    let conv_id = format!("chan_conv_{region}");
    let conv = Convection {
        id: conv_id.clone(),
        region: region.to_string(),
        h: report.mean_h_fsm_per_mm,
        t_inf: report.mean_bulk_temp_c,
        case: thermal_case.to_string(),
    };
    if let Some(existing) = doc.convections.iter_mut().find(|c| c.id == conv_id) {
        *existing = conv;
    } else {
        doc.convections.push(conv);
    }

    if let Some(pcase) = pressure_case {
        let load_id = format!("chan_press_{region}");
        let load = Load {
            id: load_id.clone(),
            region: region.to_string(),
            kind: LoadKind::Pressure {
                mpa: report.mean_pressure_mpa.max(0.0),
            },
            case: pcase.to_string(),
        };
        if let Some(existing) = doc.loads.iter_mut().find(|l| l.id == load_id) {
            *existing = load;
        } else {
            doc.loads.push(load);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{analyse, Resolution};
    use fieldsmith_doc::fsm;

    fn zero_clock() -> f64 {
        0.0
    }

    #[test]
    fn cylindrical_pipe_bore_matches_exact_area_hydraulic_diameter_and_poiseuille_drop() {
        // Solid pipe outer R_out = 10 mm, inner bore R = 5 mm (D = 10 mm), length L = 100 mm.
        let src = "\
fieldsmith 1
name \"Circular Pipe Bore\"
material ss316l
cylinder outer r=10 h=100 at=(0, 0, 50)
cylinder bore r=5 h=104 at=(0, 0, 50) subtract
";
        let doc = fsm::parse(src).expect("parse");
        // Pick laminar flow: mdot = 0.005 kg/s, rho = 1000 kg/m3, mu = 0.001 Pa*s
        // Re = 4 * mdot / (pi * D * mu) = 4 * 0.005 / (pi * 0.01 * 0.001) = 636.6 < 2100!
        let spec = ChannelFlowSpec {
            axis: ChannelAxis::Z,
            passage_mode: PassageMode::InteriorVoid,
            stations: 21,
            span_mm: Some([5.0, 95.0]), // ΔL = 90 mm = 0.09 m
            slice_h_mm: Some(0.25),
            mass_flow_kg_s: 0.005,
            density_kg_m3: 1000.0,
            viscosity_pa_s: 1.0e-3,
            conductivity_w_m_k: 0.60,
            cp_j_kg_k: 4180.0,
            roughness_mm: 0.0,
            inlet_pressure_mpa: 0.20,
            inlet_temp_c: 20.0,
            wall_temp_c: 80.0,
            node_id: None,
        };
        let rep = analyze_sdf_channel(&doc, &spec).expect("channel solve");
        let exact_area = std::f64::consts::PI * 25.0; // 78.5398 mm^2
        let exact_dh = 10.0; // 2 * R = 10 mm
        assert!(
            (rep.min_area_mm2 - exact_area).abs() / exact_area < 0.005,
            "area {} vs exact {}",
            rep.min_area_mm2,
            exact_area
        );
        assert!(
            (rep.mean_hydraulic_diameter_mm - exact_dh).abs() / exact_dh < 0.005,
            "D_h {} vs exact {}",
            rep.mean_hydraulic_diameter_mm,
            exact_dh
        );

        // Exact Hagen-Poiseuille pressure drop over L = 0.09 m:
        // Q = mdot / rho = 5e-6 m^3/s, D = 0.01 m => dp = 128 * mu * L * Q / (pi * D^4)
        let q_m3_s = 0.005 / 1000.0;
        let dp_exact_mpa =
            (128.0 * 1.0e-3 * 0.09 * q_m3_s / (std::f64::consts::PI * 0.01_f64.powi(4))) * 1e-6;
        let rel_dp_err =
            (rep.viscous_pressure_drop_mpa - dp_exact_mpa).abs() / dp_exact_mpa.max(1e-18);
        assert!(
            rel_dp_err < 0.015,
            "Poiseuille drop {} MPa vs exact {} MPa (rel err {:.3}%)",
            rep.viscous_pressure_drop_mpa,
            dp_exact_mpa,
            rel_dp_err * 100.0
        );
    }

    #[test]
    fn converging_diverging_bore_accelerates_at_throat_and_binds_to_3d_fea() {
        let src = "\
fieldsmith 1
name \"Cooled Venturi Liner\"
material cucrzr
cylinder shell r=16 h=60 at=(0, 0, 30)
revolve bore angle=360 profile=[(0, 0), (8, 0), (4.5, 30), (8, 60), (0, 60)] subtract
region inner_wall = bore.all
region outer_wall = shell.side
region mount = shell.bottom
support fix_mount mount fixed=xyz
heatflux hot_wall outer_wall q=0.8 case=\"HotFire\"
solve tref=20
";
        let mut doc = fsm::parse(src).expect("parse");
        let spec = ChannelFlowSpec {
            axis: ChannelAxis::Z,
            stations: 25,
            span_mm: Some([4.0, 56.0]),
            slice_h_mm: Some(0.35),
            mass_flow_kg_s: 0.18,
            ..Default::default()
        };
        let rep = analyze_sdf_channel(&doc, &spec).expect("venturi channel");
        // Throat at z=30 has R=4.5 => A = pi * 20.25 = 63.62 mm^2; ends have R ~ 7.5 => A ~ 178 mm^2
        assert!(rep.min_area_mm2 < 66.0 && rep.min_area_mm2 > 60.0, "min_area={}", rep.min_area_mm2);
        assert!(rep.max_area_mm2 > 160.0, "max_area={}", rep.max_area_mm2);

        let inlet = rep.stations.first().unwrap();
        let throat = rep
            .stations
            .iter()
            .min_by(|a, b| a.area_mm2.partial_cmp(&b.area_mm2).unwrap())
            .unwrap();
        assert!(throat.velocity_m_s > 2.4 * inlet.velocity_m_s);
        assert!(throat.h_conv_w_m2_k > 1.8 * inlet.h_conv_w_m2_k);

        // Bind extracted convection + fluid pressure back into `doc` and run 3D P2 FEA!
        bind_channel_bcs_to_region(&mut doc, &rep, "inner_wall", "HotFire", Some("InternalPressure"))
            .expect("bind BCs");
        let fea = analyse(&doc, Resolution::Dofs(10_000), zero_clock).expect("3D FEA with bound channel BCs");
        assert_eq!(fea.cases.len(), 2);
        assert!(fea.cases.iter().all(|c| c.solve.converged && c.max_vm_elem > 0.0));
    }
}
