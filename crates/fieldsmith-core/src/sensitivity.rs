//! General 3D parameter sensitivity operator (`∂Φ / ∂p`) for arbitrary `.fsm` documents.
//!
//! Evaluates derivatives of 3D P2 finite-element observables (compliance `C`, mass `m`,
//! peak displacement `u_max`, peak element von Mises stress `σ_vM`, peak conduction
//! temperature `T_max`, natural frequencies `f_n`, and graded TPMS lattice yield / buckling
//! utilizations `U_y`, `U_b`) with respect to document `let` variables, `system` designs,
//! `fill.*` lattice grading parameters, or dotted `ParamAddr` addresses.
//!
//! ## Differentiation Regimes (`SensitivityReport::method`)
//! 1. `"boundary_velocity_adjoint"` — Default for solid linear-elastic, coupled steady-conduction
//!    thermo-elastic, modal, AND stress-graded TPMS `fill` models (including anisotropic cubic
//!    stiffness tensors, spatially varying `SpatialFn` `lerp(...)` BCs, and declarative `.fsm`
//!    `system` blocks). Reconstructs the 3D domain velocity field `V_k` on the single baseline P2
//!    mesh from per-facet interior SDF normal velocities `V_n = -(∂d/∂p_k) / |∇d_0|` via a 3×3
//!    symmetric quadric solve at boundary corners plus harmonic interior vertex smoothing, and
//!    differentiates homogenised + sub-voxel skin-occupancy material fractions smoothly on the
//!    locked grade grid. Mechanical cases solve discrete adjoints reusing one `TwoLevel` coarse
//!    Cholesky factorization (`O(1)` linear solves in parameter count `k`, zero re-meshing).
//! 2. `"boundary_velocity_fixed_mesh"` — Used when nonlinear `J2` elastoplasticity (`solve
//!    plasticity=j2`) is active on solid models. Deforms the baseline P2 mesh along `±ε V_k` with
//!    locked region facet sets and evaluates the nonlinear elastoplastic cases on the fixed
//!    topology (zero re-meshing).
//! 3. `"finite_difference_remesh"` — Fallback when a degenerate perturbation cannot preserve
//!    positive element volumes on the fixed topology.

use crate::contracts::required_modes_for_asserts;
use crate::declarative_system::resolve_system_doc;
use crate::regions::{jacobi3, region_facets, region_facets_exact, Facet};
use crate::{
    analyse_full, case_convections_all_with_regions, case_force_report_with_regions,
    case_heat_fluxes_all_with_regions, case_names, case_temperatures_with_regions,
    collect_labelled_volumes, material_of, prescribed_fn, run_case_elastoplastic,
    run_thermal_case_multi_with_regions, structural_doc, Analysis, CaseResult, Clock, RegionLookup,
    Resolution, SetupError,
};
use fieldsmith_doc::param::{self, ParamAddr};
use fieldsmith_doc::system::DeclBindAttr;
use fieldsmith_doc::{materials, Document, LoadKind, NodeKind, PlasticityLaw};
use fieldsmith_field::expr;
use fieldsmith_solve::assemble::{element_stiffness, RefData};
use fieldsmith_solve::{
    assemble_conduction_system_spatial, element_stress_mat, min_compressive,
    modal_mass_quadratic_form, solve_conduction_multi_rhs, solve_multi_rhs, thermal_load_multi,
    thermal_load_transpose_multi, von_mises, Material, P2Mesh, SolverOptions, ThermalRefData, V3,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Sensitivity derivatives of a single solved load or thermal case with respect to one parameter `p`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaseSensitivity {
    pub case: String,
    /// `∂C / ∂p` (N·mm per unit of `p`).
    pub d_compliance: f64,
    /// Dimensionless normalized compliance elasticity `(p / C) · (∂C / ∂p)`.
    /// For an Euler–Bernoulli cantilever of depth `h`, `(h / C)(∂C / ∂h) = -3`.
    pub rel_compliance: f64,
    /// `∂u_max / ∂p` (mm per unit of `p`).
    pub d_max_displacement: f64,
    /// `∂σ_vM / ∂p` (MPa per unit of `p`, element-centroid von Mises).
    pub d_max_vm_elem: f64,
    /// `∂T_max / ∂p` (°C per unit of `p`, peak nodal conduction temperature on thermal cases).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d_max_temp: Option<f64>,
    /// `∂U_y / ∂p` (per unit of `p`, peak non-void graded lattice yield utilization `σ_vM / σ_y*(φ)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d_yield_utilization: Option<f64>,
    /// `∂U_b / ∂p` (per unit of `p`, peak lattice elastic-buckling utilization `|σ_min| / σ_el*(φ)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d_buckling_utilization: Option<f64>,
}

/// Sensitivity entry for one named `let` variable or `ParamAddr`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParameterSensitivity {
    pub param: String,
    pub nominal: f64,
    pub step: f64,
    pub scheme: String,
    /// `∂m / ∂p` (g per unit of `p`; graded mass when `doc.fill` is active).
    pub d_mass_g: f64,
    /// Dimensionless normalized mass elasticity `(p / m) · (∂m / ∂p)`.
    pub rel_mass: f64,
    /// `∂m_solid / ∂p` (g per unit of `p`, un-filled solid body mass derivative when `doc.fill` is active).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d_solid_mass_g: Option<f64>,
    /// `∂f_n / ∂p` (Hz per unit of `p`) for each solved vibration mode `n = 1..count`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub d_modes_hz: Vec<f64>,
    /// Dimensionless normalized modal frequency elasticities `(p / f_n) · (∂f_n / ∂p)`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rel_modes: Vec<f64>,
    pub cases: Vec<CaseSensitivity>,
}

/// Full sensitivity report across a document's `let` parameters (or requested parameter list).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SensitivityReport {
    /// Differentiation method provenance:
    /// - `"boundary_velocity_adjoint"` (fixed P2 topology, discrete adjoints, `O(1)` solves in `k`)
    /// - `"boundary_velocity_fixed_mesh"` (fixed P2 topology along `V_k`, nonlinear J2)
    /// - `"finite_difference_remesh"` (locked baseline `h`, re-stuffed mesh fallback)
    pub method: String,
    /// Total 3D FEA solves executed (`O(1)` independent of parameter count `k` for
    /// `"boundary_velocity_adjoint"`).
    pub solves_run: usize,
    pub h: f64,
    pub nodes: usize,
    pub elements: usize,
    pub baseline_mass_g: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_solid_mass_g: Option<f64>,
    pub baseline_compliance: Vec<(String, f64)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub baseline_modes_hz: Vec<f64>,
    pub parameters: Vec<ParameterSensitivity>,
    pub elapsed_ms: f64,
}

fn rel_elasticity(p: f64, f0: f64, df_dp: f64) -> f64 {
    if f0.abs() > 1e-15 && p.abs() > 1e-15 {
        (p / f0) * df_dp
    } else {
        0.0
    }
}

#[inline]
fn is_fixed_dof(fixed: &[u8], node: usize, comp: usize) -> bool {
    fixed[node] & (1 << comp) != 0
}

fn zero_fixed_dofs(v: &mut [f64], fixed: &[u8]) {
    for (node, &mask) in fixed.iter().enumerate() {
        if mask != 0 {
            for c in 0..3 {
                if mask & (1 << c) != 0 {
                    v[3 * node + c] = 0.0;
                }
            }
        }
    }
}

fn dot_slice(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Consistent finite-element mass (grams) integrated directly over a tetrahedral mesh.
fn mesh_mass_grams(doc: &Document, a: &Analysis) -> f64 {
    let sdoc = structural_doc(doc);
    let doc = sdoc.as_ref();
    let base_rho = materials::find(doc, &doc.material)
        .map(|m| m.density)
        .unwrap_or(1.0);
    let mut labelled = Vec::new();
    let _ = collect_labelled_volumes(doc, &doc.root, &[], &mut labelled);
    let h = a.provenance.element_size;
    let mut mass_mm3_g_cm3 = 0.0f64;
    for (e, tet) in a.mesh.tets.iter().enumerate() {
        let vol = a.mesh.tet_volume(e).abs();
        let rho = if labelled.is_empty() {
            base_rho
        } else {
            let v0 = a.mesh.nodes[tet[0] as usize];
            let v1 = a.mesh.nodes[tet[1] as usize];
            let v2 = a.mesh.nodes[tet[2] as usize];
            let v3 = a.mesh.nodes[tet[3] as usize];
            let c = [
                0.25 * (v0[0] + v1[0] + v2[0] + v3[0]),
                0.25 * (v0[1] + v1[1] + v2[1] + v3[1]),
                0.25 * (v0[2] + v1[2] + v2[2] + v3[2]),
            ];
            let mut r = base_rho;
            for (_, le, lmat) in &labelled {
                if expr::eval(le, c).v <= 1e-6 * h {
                    r = lmat.density;
                }
            }
            r
        };
        mass_mm3_g_cm3 += vol * rho;
    }
    mass_mm3_g_cm3 / 1000.0
}

/// Lock each baseline element's owning material ID (`None` = `doc.material`, `Some(mid)` = labelled
/// additive volume) so fixed-topology perturbations never flip element material ownership across
/// internal interfaces while still differentiating custommaterial property changes smoothly.
fn lock_element_material_ids(
    sdoc: &Document,
    mesh: &P2Mesh,
    h: f64,
) -> Result<Vec<Option<String>>, SetupError> {
    let mut labelled = Vec::new();
    collect_labelled_volumes(sdoc, &sdoc.root, &[], &mut labelled)?;
    if labelled.is_empty() {
        return Ok(vec![None; mesh.tets.len()]);
    }
    let mut out = Vec::with_capacity(mesh.tets.len());
    for tet in &mesh.tets {
        let v0 = mesh.nodes[tet[0] as usize];
        let v1 = mesh.nodes[tet[1] as usize];
        let v2 = mesh.nodes[tet[2] as usize];
        let v3 = mesh.nodes[tet[3] as usize];
        let c = [
            0.25 * (v0[0] + v1[0] + v2[0] + v3[0]),
            0.25 * (v0[1] + v1[1] + v2[1] + v3[1]),
            0.25 * (v0[2] + v1[2] + v2[2] + v3[2]),
        ];
        let mut chosen = None;
        for (mid, vexpr, _) in &labelled {
            if expr::eval(vexpr, c).v <= 1e-6 * h {
                chosen = Some(mid.clone());
            }
        }
        out.push(chosen);
    }
    Ok(out)
}

struct ElementMaterialProps {
    base_mat: materials::Material,
    young: Vec<f64>,
    yield_elem: Vec<f64>,
    k_elem: Vec<f64>,
    alpha_elem: Vec<Option<f64>>,
    rho_elem: Vec<f64>,
}

fn eval_element_materials(
    sdoc: &Document,
    locked_ids: &[Option<String>],
) -> Result<ElementMaterialProps, SetupError> {
    let base_mat = material_of(sdoc)?;
    let ne = locked_ids.len();
    let base_k = base_mat.k.unwrap_or(1.0).max(1e-9);
    let mut young = Vec::with_capacity(ne);
    let mut yield_elem = Vec::with_capacity(ne);
    let mut k_elem = Vec::with_capacity(ne);
    let mut alpha_elem = Vec::with_capacity(ne);
    let mut rho_elem = Vec::with_capacity(ne);
    for slot in locked_ids {
        let m = match slot {
            None => base_mat.clone(),
            Some(mid) => materials::find(sdoc, mid)
                .ok_or_else(|| SetupError(format!("unknown material '{mid}'")))?,
        };
        young.push(m.e);
        yield_elem.push(m.yield_strength);
        k_elem.push(m.k.unwrap_or(base_k) / base_k);
        alpha_elem.push(m.alpha);
        rho_elem.push(m.density);
    }
    Ok(ElementMaterialProps {
        base_mat,
        young,
        yield_elem,
        k_elem,
        alpha_elem,
        rho_elem,
    })
}

fn fixed_mesh_mass_grams(mesh: &P2Mesh, rho_elem: &[f64]) -> f64 {
    let mut mass_mm3_g_cm3 = 0.0f64;
    for (e, &rho) in rho_elem.iter().enumerate() {
        mass_mm3_g_cm3 += mesh.tet_volume(e).abs() * rho;
    }
    mass_mm3_g_cm3 / 1000.0
}

fn is_fill_param_target(doc: &Document, addr: &ParamAddr) -> bool {
    addr.id == "fill"
        && !addr.attr.is_empty()
        && doc.find("fill").is_none()
        && doc.custom_materials.iter().all(|m| m.id != "fill")
        && doc.loads.iter().all(|l| l.id != "fill")
        && doc.temperatures.iter().all(|t| t.id != "fill")
        && doc.convections.iter().all(|c| c.id != "fill")
        && doc.heat_fluxes.iter().all(|f| f.id != "fill")
}

pub(crate) fn read_param_value(doc: &Document, addr: &ParamAddr) -> Result<f64, String> {
    if addr.attr.is_empty() && !doc.lets.iter().any(|l| l.name == addr.id) {
        if let Some(sb) = &doc.system_block {
            if let Some(dp) = sb.designs.iter().find(|d| d.name == addr.id) {
                return Ok(dp.nominal);
            }
        }
    }
    if is_fill_param_target(doc, addr) {
        let f = doc
            .fill
            .as_ref()
            .ok_or_else(|| format!("param {addr}: document has no fill"))?;
        return match (addr.attr.as_str(), addr.comp) {
            ("rho_min", None) | ("rho", Some(0)) => Ok(f.rho_min),
            ("rho_max", None) | ("rho", Some(1)) => Ok(f.rho_max),
            ("skin", None) => Ok(f.skin),
            ("skin_max", None) => Ok(f.skin_max.unwrap_or(f.skin)),
            ("cell", None) => Ok(f.cell),
            ("cell_min", None) => Ok(f.cell_min.unwrap_or(f.cell)),
            ("curve", None) => Ok(f.curve),
            ("wall" | "min_wall", None) => Ok(f.min_wall),
            ("margin", None) => Ok(f.margin),
            ("erode", None) => Ok(f.erode.unwrap_or(0.0)),
            ("rotate", Some(i)) if i < 3 => Ok(f.rotate[i]),
            _ => Err(format!("param {addr}: unsupported fill parameter")),
        };
    }
    if !addr.attr.is_empty() && addr.comp.is_none() {
        if let Some(m) = doc.custom_materials.iter().find(|m| m.id == addr.id) {
            return match addr.attr.as_str() {
                "e" | "young" => Ok(m.e),
                "nu" | "poisson" => Ok(m.nu),
                "yield" | "yield_strength" => Ok(m.yield_strength),
                "density" | "rho" => Ok(m.density),
                "k" | "conductivity" => {
                    m.k.ok_or_else(|| format!("param {addr}: custommaterial has no k"))
                }
                "a" | "alpha" => m
                    .alpha
                    .ok_or_else(|| format!("param {addr}: custommaterial has no alpha")),
                _ => Err(format!(
                    "param {addr}: custommaterial {:?} has no numeric parameter {:?}",
                    addr.id, addr.attr
                )),
            };
        }
    }
    param::resolve(doc, addr)?;
    if addr.attr.is_empty() {
        return doc
            .lets
            .iter()
            .find(|l| l.name == addr.id)
            .map(|l| l.value)
            .ok_or_else(|| format!("param {addr}: unknown let {:?}", addr.id));
    }
    if let Some(node) = doc.find(&addr.id) {
        match addr.attr.as_str() {
            "at" => return Ok(node.transform.translate[addr.comp.unwrap_or(0)]),
            "rotate" => return Ok(node.transform.rotate[addr.comp.unwrap_or(0)]),
            "blend" => return Ok(node.blend),
            _ => {}
        }
        return match (&node.kind, addr.attr.as_str(), addr.comp) {
            (NodeKind::Box { size, .. }, "size", Some(i)) => Ok(size[i]),
            (NodeKind::Box { round, .. }, "round", None) => Ok(*round),
            (NodeKind::Cylinder { radius, .. }, "radius" | "r", None) => Ok(*radius),
            (NodeKind::Cylinder { height, .. }, "height" | "h", None) => Ok(*height),
            (NodeKind::Cylinder { round, .. }, "round", None) => Ok(*round),
            (NodeKind::Sphere { radius }, "radius" | "r", None) => Ok(*radius),
            (NodeKind::Capsule { radius, .. }, "radius" | "r", None) => Ok(*radius),
            (NodeKind::Capsule { length, .. }, "length" | "l", None) => Ok(*length),
            (NodeKind::Torus { major, .. }, "major", None) => Ok(*major),
            (NodeKind::Torus { minor, .. }, "minor", None) => Ok(*minor),
            (NodeKind::Extrude { height, .. }, "height" | "h", None) => Ok(*height),
            (NodeKind::Revolve { angle, .. }, "angle", None) => Ok(*angle),
            _ => Err(format!("param {addr}: unsupported node attribute")),
        };
    }
    if let Some(load) = doc.loads.iter().find(|l| l.id == addr.id) {
        return match (&load.kind, addr.attr.as_str(), addr.comp) {
            (LoadKind::Force { vector }, "force", Some(i)) => Ok(vector[i]),
            (LoadKind::Pressure { mpa, .. }, "mpa", None) => Ok(*mpa),
            (LoadKind::Preload { newtons, .. }, "newtons", None) => Ok(*newtons),
            (LoadKind::Distributed { total: Some(t), .. }, "total", None) => Ok(*t),
            _ => Err(format!("param {addr}: unsupported load attribute")),
        };
    }
    if let Some(t) = doc.temperatures.iter().find(|t| t.id == addr.id) {
        if addr.attr == "celsius" {
            return Ok(t.celsius);
        }
    }
    if let Some(c) = doc.convections.iter().find(|c| c.id == addr.id) {
        return match addr.attr.as_str() {
            "h" => Ok(c.h),
            "tinf" => Ok(c.t_inf),
            _ => Err(format!("param {addr}: unsupported convection attribute")),
        };
    }
    if let Some(f) = doc.heat_fluxes.iter().find(|f| f.id == addr.id) {
        if addr.attr == "q" {
            return Ok(f.q);
        }
    }
    Err(format!(
        "param {addr}: could not read current numeric value"
    ))
}

pub(crate) fn apply_sens_param(
    doc: &mut Document,
    addr: &ParamAddr,
    val: f64,
) -> Result<(), String> {
    if !val.is_finite() {
        return Err(format!("param {addr}: value {val} is not finite"));
    }
    if addr.attr.is_empty() && !doc.lets.iter().any(|l| l.name == addr.id) {
        if let Some(sb) = &mut doc.system_block {
            if let Some(dp) = sb.designs.iter_mut().find(|d| d.name == addr.id) {
                dp.nominal = val;
                return Ok(());
            }
        }
    }
    if is_fill_param_target(doc, addr) {
        let f = doc
            .fill
            .as_mut()
            .ok_or_else(|| format!("param {addr}: document has no fill"))?;
        return match (addr.attr.as_str(), addr.comp) {
            ("rho_min", None) | ("rho", Some(0)) => {
                if val <= 0.0 || val > f.rho_max || val > 1.0 {
                    return Err(format!(
                        "param {addr}: fill rho_min must satisfy 0 < rho_min <= rho_max <= 1"
                    ));
                }
                f.rho_min = val;
                Ok(())
            }
            ("rho_max", None) | ("rho", Some(1)) => {
                if val <= 0.0 || val < f.rho_min || val > 1.0 {
                    return Err(format!(
                        "param {addr}: fill rho_max must satisfy 0 < rho_min <= rho_max <= 1"
                    ));
                }
                f.rho_max = val;
                Ok(())
            }
            ("skin", None) => {
                if val < 0.0 || f.skin_max.is_some_and(|sm| val > sm) {
                    return Err(format!(
                        "param {addr}: fill skin must be >= 0 and <= skin_max"
                    ));
                }
                f.skin = val;
                Ok(())
            }
            ("skin_max", None) => {
                if val < f.skin {
                    return Err(format!("param {addr}: fill skin_max must be >= skin"));
                }
                f.skin_max = Some(val);
                Ok(())
            }
            ("cell", None) => {
                if val <= 0.0 || f.cell_min.is_some_and(|cm| cm > val) {
                    return Err(format!(
                        "param {addr}: fill cell must be > 0 and >= cell_min"
                    ));
                }
                f.cell = val;
                Ok(())
            }
            ("cell_min", None) => {
                if val <= 0.0 || val > f.cell {
                    return Err(format!(
                        "param {addr}: fill cell_min must satisfy 0 < cell_min <= cell"
                    ));
                }
                f.cell_min = Some(val);
                Ok(())
            }
            ("curve", None) => {
                if val <= 0.0 {
                    return Err(format!("param {addr}: fill curve must be > 0"));
                }
                f.curve = val;
                Ok(())
            }
            ("wall" | "min_wall", None) => {
                if val < 0.0 {
                    return Err(format!("param {addr}: fill min_wall must be >= 0"));
                }
                f.min_wall = val;
                Ok(())
            }
            ("margin", None) => {
                if val < 0.0 {
                    return Err(format!("param {addr}: fill margin must be >= 0"));
                }
                f.margin = val;
                Ok(())
            }
            ("erode", None) => {
                if val < 0.0 {
                    return Err(format!("param {addr}: fill erode must be >= 0"));
                }
                f.erode = Some(val);
                Ok(())
            }
            ("rotate", Some(i)) if i < 3 => {
                f.rotate[i] = val;
                Ok(())
            }
            _ => Err(format!("param {addr}: unsupported fill parameter")),
        };
    }
    if !addr.attr.is_empty() && addr.comp.is_none() {
        if let Some(m) = doc.custom_materials.iter_mut().find(|m| m.id == addr.id) {
            return match addr.attr.as_str() {
                "e" | "young" => {
                    if val <= 0.0 {
                        return Err(format!("param {addr}: Young's modulus must be > 0"));
                    }
                    m.e = val;
                    Ok(())
                }
                "nu" | "poisson" => {
                    if !(0.0..0.499).contains(&val) {
                        return Err(format!("param {addr}: Poisson ratio must be in [0, 0.499)"));
                    }
                    m.nu = val;
                    Ok(())
                }
                "yield" | "yield_strength" => {
                    m.yield_strength = val;
                    Ok(())
                }
                "density" | "rho" => {
                    if val <= 0.0 {
                        return Err(format!("param {addr}: density must be > 0"));
                    }
                    m.density = val;
                    Ok(())
                }
                "k" | "conductivity" => {
                    if val <= 0.0 {
                        return Err(format!("param {addr}: conductivity must be > 0"));
                    }
                    m.k = Some(val);
                    Ok(())
                }
                "a" | "alpha" => {
                    m.alpha = Some(val);
                    Ok(())
                }
                _ => Err(format!(
                    "param {addr}: custommaterial {:?} has no numeric parameter {:?}",
                    addr.id, addr.attr
                )),
            };
        }
    }
    param::apply(doc, addr, val)?;
    if addr.attr.is_empty() {
        if let Some(l) = doc.lets.iter_mut().find(|l| l.name == addr.id) {
            l.expr = format!("{val}");
        }
        if let Some(sb) = &mut doc.system_block {
            if let Some(dp) = sb.designs.iter_mut().find(|d| d.name == addr.id) {
                dp.nominal = val;
            }
        }
    }
    Ok(())
}

pub(crate) fn default_target_params(doc: &Document, params: &[String]) -> Vec<String> {
    if !params.is_empty() {
        return params.to_vec();
    }
    let bound_lets: HashSet<&str> = doc
        .system_block
        .as_ref()
        .map(|sb| {
            sb.bindings
                .iter()
                .filter(|b| b.attr == DeclBindAttr::Let)
                .map(|b| b.target.as_str())
                .collect()
        })
        .unwrap_or_default();
    let mut names = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for l in &doc.lets {
        let is_derived = seen.iter().any(|prev| l.expr.contains(prev.as_str()));
        seen.push(l.name.clone());
        if is_derived || bound_lets.contains(l.name.as_str()) {
            continue;
        }
        names.push(l.name.clone());
    }
    if let Some(sb) = &doc.system_block {
        for dp in &sb.designs {
            if !bound_lets.contains(dp.name.as_str()) && !names.iter().any(|n| n == &dp.name) {
                names.push(dp.name.clone());
            }
        }
    }
    if doc.fill.is_some() && doc.find("fill").is_none() {
        for fp in ["fill.rho_min", "fill.rho_max", "fill.skin", "fill.cell"] {
            if !names.iter().any(|n| n == fp) {
                names.push(fp.to_string());
            }
        }
    }
    names
}

fn build_vertex_adjacency(mesh: &P2Mesh) -> Vec<Vec<u32>> {
    let mut adj = vec![Vec::<u32>::new(); mesh.nverts];
    const EDGES: [(usize, usize); 6] = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
    for tet in &mesh.tets {
        for &(a, b) in &EDGES {
            let va = tet[a];
            let vb = tet[b];
            if !adj[va as usize].contains(&vb) {
                adj[va as usize].push(vb);
            }
            if !adj[vb as usize].contains(&va) {
                adj[vb as usize].push(va);
            }
        }
    }
    adj
}

/// Reconstruct a smooth 3D mesh velocity `V(x) = dx/dp` on all `base.mesh` P2 nodes:
/// 1. Probe interior points `x_probe = (1 - τ) x_corner + τ x_centroid` (`τ = 0.02`) on each
///    boundary triangle so sharp CSG edges/corners never hit `min`/`max` kinks while curved faces
///    evaluate `∇d_0` within `0.02` of the vertex angle.
/// 2. Accumulate the area-weighted 3×3 symmetric normal quadric `M_i V_i = b_i` at each boundary
///    vertex `i < nverts` and solve via truncated eigenspace pseudoinverse (`eig[m] > 5e-3 * eig[0]`).
/// 3. Harmonically extend `V` to interior tetrahedral vertices (`20` Jacobi sweeps) and set P2
///    midside edge nodes to `0.5 * (V_a + V_b)` so elements stay strictly affine.
#[allow(clippy::too_many_arguments)]
fn compute_mesh_velocity(
    doc: &Document,
    addr: &ParamAddr,
    p0: f64,
    e0: &expr::Expr,
    mesh: &P2Mesh,
    facets: &[Facet],
    vert_adj: &[Vec<u32>],
    base_h: f64,
) -> Result<Vec<V3>, SetupError> {
    let mut step_sdf = 1e-6 * p0.abs().max(1.0);
    let build_sdf_pair = |delta: f64| -> Result<(expr::Expr, expr::Expr, f64), SetupError> {
        let mut d_hi = doc.clone();
        let mut d_lo = doc.clone();
        let ok_hi = apply_sens_param(&mut d_hi, addr, p0 + delta);
        let ok_lo = apply_sens_param(&mut d_lo, addr, p0 - delta);
        match (ok_hi, ok_lo) {
            (Ok(()), Ok(())) => {
                let rhi = resolve_system_doc(&d_hi)?;
                let rlo = resolve_system_doc(&d_lo)?;
                let shi = structural_doc(rhi.as_ref());
                let slo = structural_doc(rlo.as_ref());
                Ok((
                    expr::compile_body(shi.as_ref()),
                    expr::compile_body(slo.as_ref()),
                    2.0 * delta,
                ))
            }
            (Ok(()), Err(_)) => {
                let rhi = resolve_system_doc(&d_hi)?;
                let shi = structural_doc(rhi.as_ref());
                Ok((expr::compile_body(shi.as_ref()), e0.clone(), delta))
            }
            (Err(_), Ok(())) => {
                let rlo = resolve_system_doc(&d_lo)?;
                let slo = structural_doc(rlo.as_ref());
                Ok((e0.clone(), expr::compile_body(slo.as_ref()), delta))
            }
            (Err(e), Err(_)) => Err(SetupError(e)),
        }
    };

    let (mut e_hi, mut e_lo, mut denom) = build_sdf_pair(step_sdf)?;

    // Ensure SDF level-set shift stays strictly smaller than the facet-interior probe offset
    // `τ * h / 3` (`τ = 0.02`), and short-circuit immediately if `addr` does not move geometry.
    let mut max_dd = 0.0f64;
    for fct in facets {
        let pc = fct.centroid(mesh);
        let dd = (expr::eval(&e_hi, pc).v - expr::eval(&e_lo, pc).v).abs();
        if dd > max_dd {
            max_dd = dd;
        }
    }
    if max_dd == 0.0 {
        return Ok(vec![[0.0; 3]; mesh.nodes.len()]);
    }
    let limit_dd = 1e-3 * base_h.max(1e-4);
    if max_dd > limit_dd {
        let scale = (0.1 * limit_dd) / max_dd;
        step_sdf *= scale;
        if step_sdf > 0.0 {
            let pair = build_sdf_pair(step_sdf)?;
            e_hi = pair.0;
            e_lo = pair.1;
            denom = pair.2;
        }
    }

    let nv = mesh.nverts;
    let mut m_quad = vec![[[0.0f64; 3]; 3]; nv];
    let mut b_quad = vec![[0.0f64; 3]; nv];
    let mut is_bdy = vec![false; nv];

    const TAU: f64 = 0.02;
    for fct in facets {
        let a_f = fct.area(mesh);
        if a_f <= 1e-18 {
            continue;
        }
        let corners = fct.corners(mesh);
        let pc = fct.centroid(mesh);
        let n_facet = fct.normal(mesh);
        for (loc, &pi) in corners.iter().enumerate() {
            let vi = fct.nodes[loc] as usize;
            is_bdy[vi] = true;
            let x_probe = [
                (1.0 - TAU) * pi[0] + TAU * pc[0],
                (1.0 - TAU) * pi[1] + TAU * pc[1],
                (1.0 - TAU) * pi[2] + TAU * pc[2],
            ];
            let g0 = expr::eval(e0, x_probe).g;
            let gnorm = (g0[0] * g0[0] + g0[1] * g0[1] + g0[2] * g0[2]).sqrt();
            let align = if gnorm > 1e-6 {
                (g0[0] * n_facet[0] + g0[1] * n_facet[1] + g0[2] * n_facet[2]) / gnorm
            } else {
                0.0
            };
            let (n_hat, vn) = if gnorm > 1e-6 && align > 0.2 {
                let dd_dp = (expr::eval(&e_hi, x_probe).v - expr::eval(&e_lo, x_probe).v) / denom;
                (
                    [g0[0] / gnorm, g0[1] / gnorm, g0[2] / gnorm],
                    -dd_dp / gnorm,
                )
            } else {
                let dd_dp = (expr::eval(&e_hi, pc).v - expr::eval(&e_lo, pc).v) / denom;
                (n_facet, -dd_dp)
            };
            for r in 0..3 {
                b_quad[vi][r] += a_f * vn * n_hat[r];
                for c in 0..3 {
                    m_quad[vi][r][c] += a_f * n_hat[r] * n_hat[c];
                }
            }
        }
    }

    let mut vel = vec![[0.0f64; 3]; mesh.nodes.len()];
    for vi in 0..nv {
        if !is_bdy[vi] {
            continue;
        }
        let (eig, vecs) = jacobi3(&m_quad[vi]);
        if eig[0] <= 1e-30 {
            continue;
        }
        let tol_eig = 5e-3 * eig[0];
        let mut v_i = [0.0f64; 3];
        for m in 0..3 {
            if eig[m] > tol_eig {
                let proj = vecs[m][0] * b_quad[vi][0]
                    + vecs[m][1] * b_quad[vi][1]
                    + vecs[m][2] * b_quad[vi][2];
                let coeff = proj / eig[m];
                for k in 0..3 {
                    v_i[k] += coeff * vecs[m][k];
                }
            }
        }
        vel[vi] = v_i;
    }

    // Harmonic Jacobi smoothing onto interior vertices (`0..nv` with `!is_bdy`).
    let mut buf = vel[..nv].to_vec();
    for _ in 0..20 {
        for vi in 0..nv {
            if is_bdy[vi] || vert_adj[vi].is_empty() {
                continue;
            }
            let mut sum = [0.0f64; 3];
            for &nb in &vert_adj[vi] {
                let vn = vel[nb as usize];
                sum[0] += vn[0];
                sum[1] += vn[1];
                sum[2] += vn[2];
            }
            let inv_deg = 1.0 / vert_adj[vi].len() as f64;
            buf[vi] = [sum[0] * inv_deg, sum[1] * inv_deg, sum[2] * inv_deg];
        }
        vel[..nv].copy_from_slice(&buf);
    }

    // Midside edge nodes sit at exact chord midpoints `0.5 * (va + vb)`.
    for (k, &[va, vb]) in mesh.mid_edge.iter().enumerate() {
        let a = vel[va as usize];
        let b = vel[vb as usize];
        vel[nv + k] = [
            0.5 * (a[0] + b[0]),
            0.5 * (a[1] + b[1]),
            0.5 * (a[2] + b[2]),
        ];
    }

    Ok(vel)
}

fn deform_mesh_checked(base_mesh: &P2Mesh, vel: &[V3], delta: f64) -> Option<P2Mesh> {
    let mut m = base_mesh.clone();
    if delta != 0.0 {
        for (p, v) in m.nodes.iter_mut().zip(vel) {
            p[0] += delta * v[0];
            p[1] += delta * v[1];
            p[2] += delta * v[2];
        }
        for e in 0..m.tets.len() {
            let v0 = base_mesh.tet_volume(e);
            let v1 = m.tet_volume(e);
            if !v1.is_finite() || v0 * v1 <= 0.0 || v1.abs() < 0.05 * v0.abs() {
                return None;
            }
        }
    }
    Some(m)
}

struct LinearCaseAdjoint {
    lambda_u: Vec<f64>,
    e_star: usize,
    lambda_vm: Vec<f64>,
    yield_util: Option<(usize, Vec<f64>)>,
    buckling_util: Option<(usize, Vec<f64>)>,
}

struct GradedAdjointCtx<'a> {
    spec: &'a fieldsmith_doc::Fill,
    yield_strength: f64,
    young_solid: f64,
    class: &'a [u8],
    rho_elem: &'a [f64],
    sigma_min_elem: &'a [f64],
}

struct ThermalCaseAdjoint {
    pure_conduction: bool,
    m_star: usize,
    e_star: usize,
    lambda_u: Vec<f64>,
    lambda_vm: Vec<f64>,
    lambda_t_comp: Vec<f64>,
    lambda_t_u: Vec<f64>,
    lambda_t_vm: Vec<f64>,
    lambda_t_max: Vec<f64>,
}

enum CaseAdjoint {
    Mechanical(LinearCaseAdjoint),
    Thermal(ThermalCaseAdjoint),
    ModalOnly,
}

fn peak_temperature_opt(t: Option<&[f64]>) -> Option<f64> {
    t.map(|vals| vals.iter().copied().fold(f64::NEG_INFINITY, f64::max))
}

fn build_linear_case_adjoint_mat(
    mesh: &P2Mesh,
    mat: &Material,
    fixed: &[u8],
    u_0: &[f64],
    vm_elem: &[f64],
    graded_ctx: Option<&GradedAdjointCtx<'_>>,
) -> Result<(LinearCaseAdjoint, usize), SetupError> {
    let ndofs = mesh.ndofs();
    let nn = mesh.nodes.len();

    // 1. Adjoint RHS `c_u = ∂‖u(n*)‖ / ∂u` at peak-displacement node `n*`.
    let mut n_star = 0usize;
    let mut u_max = -1.0f64;
    for i in 0..nn {
        let mag = (u_0[3 * i].powi(2) + u_0[3 * i + 1].powi(2) + u_0[3 * i + 2].powi(2)).sqrt();
        if mag > u_max {
            u_max = mag;
            n_star = i;
        }
    }
    let mut c_u = vec![0.0f64; ndofs];
    if u_max > 1e-30 {
        for c in 0..3 {
            if !is_fixed_dof(fixed, n_star, c) {
                c_u[3 * n_star + c] = u_0[3 * n_star + c] / u_max;
            }
        }
    }

    // 2. Adjoint RHS `c_vm = ∂σ_vM(e*) / ∂u` at peak centroid-stress element `e*`.
    let mut e_star = 0usize;
    let mut vm_max = -1.0f64;
    for (e, &vm) in vm_elem.iter().enumerate() {
        if vm > vm_max {
            vm_max = vm;
            e_star = e;
        }
    }
    let mut c_vm = vec![0.0f64; ndofs];
    if vm_max > 1e-30 && !mesh.tets.is_empty() {
        let tet = mesh.tets[e_star];
        let mut max_u_loc = 0.0f64;
        for &n in &tet {
            for c in 0..3 {
                max_u_loc = max_u_loc.max(u_0[3 * n as usize + c].abs());
            }
        }
        let du = 1e-7 * max_u_loc.max(1.0);
        let mut u_work = u_0.to_vec();
        for &n in &tet {
            let ni = n as usize;
            for c in 0..3 {
                if is_fixed_dof(fixed, ni, c) {
                    continue;
                }
                let dof = 3 * ni + c;
                let orig = u_work[dof];
                u_work[dof] = orig + du;
                let vm_hi = von_mises(&element_stress_mat(mesh, &u_work, e_star, mat, [0.25; 3]));
                u_work[dof] = orig - du;
                let vm_lo = von_mises(&element_stress_mat(mesh, &u_work, e_star, mat, [0.25; 3]));
                u_work[dof] = orig;
                c_vm[dof] = (vm_hi - vm_lo) / (2.0 * du);
            }
        }
    }

    let mut rhs_list = vec![c_u, c_vm];
    let mut uy_elem: Option<usize> = None;
    let mut ub_elem: Option<usize> = None;

    if let Some(gctx) = graded_ctx {
        if gctx.yield_strength > 0.0 && !mesh.tets.is_empty() {
            let mut e_y = None;
            let mut uy_max = 0.0f64;
            let mut sy_at_star = 0.0f64;
            for (e, &vm) in vm_elem.iter().enumerate().take(mesh.tets.len()) {
                if gctx.class[e] == crate::fill::CLASS_VOID {
                    continue;
                }
                let sy = crate::fill::strength_extended(
                    gctx.spec.family,
                    gctx.spec.mode,
                    gctx.yield_strength,
                    gctx.rho_elem[e],
                );
                if sy <= 1e-30 {
                    continue;
                }
                let r = vm / sy;
                if r > uy_max {
                    uy_max = r;
                    e_y = Some(e);
                    sy_at_star = sy;
                }
            }
            if let Some(ey) = e_y {
                let mut c_uy = vec![0.0f64; ndofs];
                let tet = mesh.tets[ey];
                let mut max_u_loc = 0.0f64;
                for &n in &tet {
                    for c in 0..3 {
                        max_u_loc = max_u_loc.max(u_0[3 * n as usize + c].abs());
                    }
                }
                let du = 1e-7 * max_u_loc.max(1.0);
                let mut u_work = u_0.to_vec();
                for &n in &tet {
                    let ni = n as usize;
                    for c in 0..3 {
                        if is_fixed_dof(fixed, ni, c) {
                            continue;
                        }
                        let dof = 3 * ni + c;
                        let orig = u_work[dof];
                        u_work[dof] = orig + du;
                        let vm_hi =
                            von_mises(&element_stress_mat(mesh, &u_work, ey, mat, [0.25; 3]));
                        u_work[dof] = orig - du;
                        let vm_lo =
                            von_mises(&element_stress_mat(mesh, &u_work, ey, mat, [0.25; 3]));
                        u_work[dof] = orig;
                        c_uy[dof] = (vm_hi - vm_lo) / (2.0 * du * sy_at_star);
                    }
                }
                uy_elem = Some(ey);
                rhs_list.push(c_uy);
            }
        }

        if !mesh.tets.is_empty() {
            let mut e_b = None;
            let mut ub_max = 0.0f64;
            let mut sb_at_star = 0.0f64;
            for (e, (&cls, &smin)) in gctx
                .class
                .iter()
                .zip(gctx.sigma_min_elem)
                .enumerate()
                .take(mesh.tets.len())
            {
                if cls != crate::fill::CLASS_LATTICE {
                    continue;
                }
                let sc = -smin;
                if sc <= 0.0 {
                    continue;
                }
                let sb = crate::fill::buckling_extended(
                    gctx.spec.family,
                    gctx.spec.mode,
                    gctx.young_solid,
                    gctx.rho_elem[e],
                );
                if sb <= 1e-30 {
                    continue;
                }
                let r = sc / sb;
                if r > ub_max {
                    ub_max = r;
                    e_b = Some(e);
                    sb_at_star = sb;
                }
            }
            if let Some(eb) = e_b {
                let mut c_ub = vec![0.0f64; ndofs];
                let tet = mesh.tets[eb];
                let mut max_u_loc = 0.0f64;
                for &n in &tet {
                    for c in 0..3 {
                        max_u_loc = max_u_loc.max(u_0[3 * n as usize + c].abs());
                    }
                }
                let du = 1e-7 * max_u_loc.max(1.0);
                let mut u_work = u_0.to_vec();
                for &n in &tet {
                    let ni = n as usize;
                    for c in 0..3 {
                        if is_fixed_dof(fixed, ni, c) {
                            continue;
                        }
                        let dof = 3 * ni + c;
                        let orig = u_work[dof];
                        u_work[dof] = orig + du;
                        let sc_hi = -min_compressive(&element_stress_mat(
                            mesh, &u_work, eb, mat, [0.25; 3],
                        ));
                        u_work[dof] = orig - du;
                        let sc_lo = -min_compressive(&element_stress_mat(
                            mesh, &u_work, eb, mat, [0.25; 3],
                        ));
                        u_work[dof] = orig;
                        c_ub[dof] = (sc_hi - sc_lo) / (2.0 * du * sb_at_star);
                    }
                }
                ub_elem = Some(eb);
                rhs_list.push(c_ub);
            }
        }
    }

    let n_rhs = rhs_list.len();
    let mut solved = solve_multi_rhs(mesh, mat, fixed, &rhs_list, SolverOptions::default())
        .map_err(SetupError)?;
    let buckling_util = ub_elem.map(|eb| (eb, solved.pop().expect("c_ub solution").0));
    let yield_util = uy_elem.map(|ey| (ey, solved.pop().expect("c_uy solution").0));
    let (lambda_vm, _) = solved.pop().expect("c_vm solution");
    let (lambda_u, _) = solved.pop().expect("c_u solution");
    Ok((
        LinearCaseAdjoint {
            lambda_u,
            e_star,
            lambda_vm,
            yield_util,
            buckling_util,
        },
        n_rhs,
    ))
}

fn build_linear_case_adjoint(
    mesh: &P2Mesh,
    young: &[f64],
    poisson: f64,
    fixed: &[u8],
    u_0: &[f64],
    vm_elem: &[f64],
) -> Result<LinearCaseAdjoint, SetupError> {
    let mat = Material::iso(young.to_vec(), poisson);
    build_linear_case_adjoint_mat(mesh, &mat, fixed, u_0, vm_elem, None).map(|(adj, _)| adj)
}

/// Precompute staggered thermo-elastic discrete adjoints (`K_u λ_u = c_u`, `K_u λ_vm = c_vm`,
/// plus scalar conduction adjoints `A_T λ_T = B_thᵀ w`, `e_{m*}` reusing one `ScalarTwoLevel`
/// Cholesky factorization). Runs in `O(1)` solves independent of parameter count `k`.
#[allow(clippy::too_many_arguments)]
fn build_thermal_case_adjoint(
    doc: &Document,
    mesh: &P2Mesh,
    facets: &[Facet],
    base_h: f64,
    case_name: &str,
    k_elem: &[f64],
    young: &[f64],
    poisson: f64,
    alphas: &[f64],
    fixed: &[u8],
    bc: &CaseResult,
    thermal_lookup: &RegionLookup<'_>,
) -> Result<(ThermalCaseAdjoint, usize), SetupError> {
    let d_0 =
        case_temperatures_with_regions(doc, mesh, facets, case_name, base_h, Some(thermal_lookup))?;
    let (robin_0, s_robin_0) = case_convections_all_with_regions(
        doc,
        mesh,
        facets,
        case_name,
        base_h,
        Some(thermal_lookup),
    )?;
    let (flux_0, s_flux_0) = case_heat_fluxes_all_with_regions(
        doc,
        mesh,
        facets,
        case_name,
        base_h,
        Some(thermal_lookup),
    )?;

    let t_0 = bc.temperature.as_deref().ok_or_else(|| {
        SetupError(format!(
            "thermal case '{case_name}' has no temperature field"
        ))
    })?;
    let mut m_star = 0usize;
    let mut t_max = f64::NEG_INFINITY;
    for (i, &tv) in t_0.iter().enumerate() {
        if tv > t_max {
            t_max = tv;
            m_star = i;
        }
    }
    let nn = mesh.nodes.len();
    let mut rhs_t_max = vec![0.0f64; nn];
    if d_0[m_star].is_none() {
        rhs_t_max[m_star] = 1.0;
    }

    let pure_conduction = fixed.iter().all(|&m| m == 0) || alphas.iter().all(|&a| a.abs() < 1e-18);
    if pure_conduction {
        let mut solved_t = solve_conduction_multi_rhs(
            mesh,
            &d_0,
            k_elem,
            &robin_0,
            &s_robin_0,
            &flux_0,
            &s_flux_0,
            &[rhs_t_max],
            SolverOptions::default(),
        )
        .map_err(SetupError)?;
        let (lambda_t_max, _) = solved_t.pop().expect("lambda_t_max");
        return Ok((
            ThermalCaseAdjoint {
                pure_conduction: true,
                m_star,
                e_star: 0,
                lambda_u: Vec::new(),
                lambda_vm: Vec::new(),
                lambda_t_comp: Vec::new(),
                lambda_t_u: Vec::new(),
                lambda_t_vm: Vec::new(),
                lambda_t_max,
            },
            1,
        ));
    }

    // Notice `element_stress_thermal` subtracts isotropic `beta * dt * I`, and von Mises stress
    // is invariant to hydrostatic pressure (`∂σ_vM / ∂T|_{u} ≡ 0`), so `build_linear_case_adjoint`
    // builds the exact mechanical adjoints `λ_u`, `λ_vm`.
    let mech_adj = build_linear_case_adjoint(mesh, young, poisson, fixed, &bc.u, &bc.vm_elem)?;
    let th_rd = ThermalRefData::new();
    let b_u0 = thermal_load_transpose_multi(mesh, &bc.u, young, poisson, alphas, &th_rd);
    let rhs_t_comp: Vec<f64> = b_u0.into_iter().map(|v| 2.0 * v).collect();
    let rhs_t_u =
        thermal_load_transpose_multi(mesh, &mech_adj.lambda_u, young, poisson, alphas, &th_rd);
    let rhs_t_vm =
        thermal_load_transpose_multi(mesh, &mech_adj.lambda_vm, young, poisson, alphas, &th_rd);

    let mut solved_t = solve_conduction_multi_rhs(
        mesh,
        &d_0,
        k_elem,
        &robin_0,
        &s_robin_0,
        &flux_0,
        &s_flux_0,
        &[rhs_t_comp, rhs_t_u, rhs_t_vm, rhs_t_max],
        SolverOptions::default(),
    )
    .map_err(SetupError)?;
    let (lambda_t_max, _) = solved_t.pop().expect("lambda_t_max");
    let (lambda_t_vm, _) = solved_t.pop().expect("lambda_t_vm");
    let (lambda_t_u, _) = solved_t.pop().expect("lambda_t_u");
    let (lambda_t_comp, _) = solved_t.pop().expect("lambda_t_comp");

    Ok((
        ThermalCaseAdjoint {
            pure_conduction: false,
            m_star,
            e_star: mech_adj.e_star,
            lambda_u: mech_adj.lambda_u,
            lambda_vm: mech_adj.lambda_vm,
            lambda_t_comp,
            lambda_t_u,
            lambda_t_vm,
            lambda_t_max,
        },
        6,
    ))
}

#[allow(clippy::too_many_arguments)]
fn assemble_conduction_lifted_residual(
    sdoc_p: &Document,
    mesh_p: &P2Mesh,
    facets: &[Facet],
    base_h: f64,
    case_name: &str,
    k_elem_p: &[f64],
    t_0: &[f64],
    thermal_lookup: &RegionLookup<'_>,
) -> Result<(Vec<f64>, Vec<f64>), SetupError> {
    let d_p = case_temperatures_with_regions(
        sdoc_p,
        mesh_p,
        facets,
        case_name,
        base_h,
        Some(thermal_lookup),
    )?;
    let (robin_p, s_robin_p) = case_convections_all_with_regions(
        sdoc_p,
        mesh_p,
        facets,
        case_name,
        base_h,
        Some(thermal_lookup),
    )?;
    let (flux_p, s_flux_p) = case_heat_fluxes_all_with_regions(
        sdoc_p,
        mesh_p,
        facets,
        case_name,
        base_h,
        Some(thermal_lookup),
    )?;
    let (a_p, q_p, fixed_t_p) = assemble_conduction_system_spatial(
        mesh_p, &d_p, k_elem_p, &robin_p, &s_robin_p, &flux_p, &s_flux_p,
    )
    .map_err(SetupError)?;
    let t_lift_p: Vec<f64> = d_p
        .iter()
        .zip(t_0)
        .map(|(dv, &t0v)| dv.unwrap_or(t0v))
        .collect();
    let mut a_tl = vec![0.0f64; t_0.len()];
    a_p.mul(&t_lift_p, &mut a_tl);
    let mut res = vec![0.0f64; t_0.len()];
    for i in 0..t_0.len() {
        if !fixed_t_p[i] {
            res[i] = q_p[i] - a_tl[i];
        }
    }
    Ok((res, t_lift_p))
}

/// Matrix-free assembly of `(K(mesh_hi, mat_hi) - K(mesh_lo, mat_lo)) / denom * u_0`
/// over all P2 tetrahedra (isotropic or cubic anisotropic TPMS elements), zeroing Dirichlet-fixed DOFs.
#[allow(clippy::too_many_arguments)]
fn stiffness_matvec_derivative(
    mesh_hi: &P2Mesh,
    mat_hi: &Material,
    mesh_lo: &P2Mesh,
    mat_lo: &Material,
    u_0: &[f64],
    fixed: &[u8],
    denom: f64,
    rd: &RefData,
) -> Vec<f64> {
    let mut dk_u = vec![0.0f64; mesh_hi.ndofs()];
    let inv_denom = 1.0 / denom;
    for (e, tet) in mesh_hi.tets.iter().enumerate() {
        let ke_hi = match mat_hi.cubic.as_ref().and_then(|cs| cs[e].as_ref()) {
            Some(c) => fieldsmith_solve::ortho::element_stiffness_cubic(mesh_hi, e, c, rd),
            None => element_stiffness(mesh_hi, e, mat_hi.young[e], mat_hi.poisson, rd),
        };
        let ke_lo = match mat_lo.cubic.as_ref().and_then(|cs| cs[e].as_ref()) {
            Some(c) => fieldsmith_solve::ortho::element_stiffness_cubic(mesh_lo, e, c, rd),
            None => element_stiffness(mesh_lo, e, mat_lo.young[e], mat_lo.poisson, rd),
        };
        let mut u_loc = [0.0f64; 30];
        for (a, &na) in tet.iter().enumerate() {
            let base_idx = 3 * na as usize;
            u_loc[3 * a] = u_0[base_idx];
            u_loc[3 * a + 1] = u_0[base_idx + 1];
            u_loc[3 * a + 2] = u_0[base_idx + 2];
        }
        for (a, &na) in tet.iter().enumerate() {
            let row_dof = 3 * na as usize;
            let mut acc = [0.0f64; 3];
            for b in 0..10 {
                let blk_hi = &ke_hi[a * 10 + b];
                let blk_lo = &ke_lo[a * 10 + b];
                let ub0 = u_loc[3 * b];
                let ub1 = u_loc[3 * b + 1];
                let ub2 = u_loc[3 * b + 2];
                for r in 0..3 {
                    acc[r] += (blk_hi[3 * r] - blk_lo[3 * r]) * ub0
                        + (blk_hi[3 * r + 1] - blk_lo[3 * r + 1]) * ub1
                        + (blk_hi[3 * r + 2] - blk_lo[3 * r + 2]) * ub2;
                }
            }
            dk_u[row_dof] += acc[0] * inv_denom;
            dk_u[row_dof + 1] += acc[1] * inv_denom;
            dk_u[row_dof + 2] += acc[2] * inv_denom;
        }
    }
    zero_fixed_dofs(&mut dk_u, fixed);
    dk_u
}

fn modal_density_ns2_mm4(mesh: &P2Mesh, rho_elem: &[f64], fallback_rho: f64) -> f64 {
    let mut weighted_rho = 0.0f64;
    let mut total_vol = 0.0f64;
    for (e, &rho) in rho_elem.iter().enumerate() {
        let tv = mesh.tet_volume(e).abs();
        weighted_rho += tv * rho;
        total_vol += tv;
    }
    let rho_avg = if total_vol > 0.0 {
        weighted_rho / total_vol
    } else {
        fallback_rho
    };
    rho_avg * 1e-9
}

/// Exact `O(0)`-extra-solve Rayleigh-quotient differentiation of mass-normalised P2 eigenmodes:
/// `∂λ_n/∂p = φ_nᵀ (∂K/∂p − λ_n ∂M/∂p) φ_n`, `∂f_n/∂p = (∂λ_n/∂p) / (8π² f_n)`.
#[allow(clippy::too_many_arguments)]
fn modal_rayleigh_sensitivities(
    modal: &fieldsmith_solve::ModalResult,
    p0: f64,
    mesh_hi: &P2Mesh,
    props_hi: &ElementMaterialProps,
    mesh_lo: &P2Mesh,
    props_lo: &ElementMaterialProps,
    fixed: &[u8],
    denom: f64,
    rd: &RefData,
) -> (Vec<f64>, Vec<f64>) {
    let rho_hi = modal_density_ns2_mm4(mesh_hi, &props_hi.rho_elem, props_hi.base_mat.density);
    let rho_lo = modal_density_ns2_mm4(mesh_lo, &props_lo.rho_elem, props_lo.base_mat.density);
    let mat_hi = Material::iso(props_hi.young.clone(), props_hi.base_mat.nu);
    let mat_lo = Material::iso(props_lo.young.clone(), props_lo.base_mat.nu);
    let n_modes = modal
        .frequencies
        .len()
        .min(modal.eigenvalues.len())
        .min(modal.shapes.len());
    let mut d_modes_hz = Vec::with_capacity(n_modes);
    let mut rel_modes = Vec::with_capacity(n_modes);
    let eight_pi_sq = 8.0 * std::f64::consts::PI * std::f64::consts::PI;
    for m in 0..n_modes {
        let phi = &modal.shapes[m];
        let lambda = modal.eigenvalues[m];
        let f_hz = modal.frequencies[m];
        let dk_phi =
            stiffness_matvec_derivative(mesh_hi, &mat_hi, mesh_lo, &mat_lo, phi, fixed, denom, rd);
        let dk_dp = dot_slice(phi, &dk_phi);
        let m_hi = modal_mass_quadratic_form(mesh_hi, rho_hi, phi);
        let m_lo = modal_mass_quadratic_form(mesh_lo, rho_lo, phi);
        let dm_dp = (m_hi - m_lo) / denom;
        let d_lambda = dk_dp - lambda * dm_dp;
        let df_hz = if f_hz > 1e-12 {
            d_lambda / (eight_pi_sq * f_hz)
        } else {
            0.0
        };
        d_modes_hz.push(df_hz);
        rel_modes.push(rel_elasticity(p0, f_hz, df_hz));
    }
    (d_modes_hz, rel_modes)
}

#[derive(Clone, Copy)]
struct FixedCaseMetrics {
    compliance: f64,
    max_displacement: f64,
    max_vm_elem: f64,
    max_temp: Option<f64>,
}

/// Evaluate all mechanical and thermal cases of `d_pert` on a fixed-topology deformed mesh
/// `mesh_pert` using locked baseline region facet sets (`0` re-meshing).
#[allow(clippy::too_many_arguments)]
fn evaluate_cases_on_fixed_mesh(
    d_pert: &Document,
    mesh_pert: &P2Mesh,
    facets: &[Facet],
    fixed: &[u8],
    base_h: f64,
    locked_elem_mats: &[Option<String>],
    exact_lookup: &RegionLookup<'_>,
    thermal_lookup: &RegionLookup<'_>,
) -> Result<(Vec<FixedCaseMetrics>, usize), SetupError> {
    let sys_resolved = resolve_system_doc(d_pert)?;
    let sdoc = structural_doc(sys_resolved.as_ref());
    let doc = sdoc.as_ref();
    let props = eval_element_materials(doc, locked_elem_mats)?;
    let nl_plastic = doc.solve.plasticity == Some(PlasticityLaw::J2);
    let hardening_n = doc
        .solve
        .hardening_n
        .or(props.base_mat.hardening_n)
        .unwrap_or(0.18);
    let mut metrics = Vec::new();
    let mut solves = 0usize;

    for name in case_names(doc) {
        let (force, _, prescribed_map) = case_force_report_with_regions(
            doc,
            mesh_pert,
            facets,
            &name,
            base_h,
            Some(exact_lookup),
        )?;
        let mut node_key: HashMap<[u64; 3], u8> = HashMap::new();
        for (i, p) in mesh_pert.nodes.iter().enumerate() {
            node_key.insert([p[0].to_bits(), p[1].to_bits(), p[2].to_bits()], fixed[i]);
        }
        let prescribed = prescribed_fn(&node_key, &prescribed_map);
        let case = if nl_plastic
            && (props.base_mat.yield_strength > 0.0 || props.yield_elem.iter().any(|&y| y > 0.0))
        {
            run_case_elastoplastic(
                mesh_pert,
                &props.young,
                props.base_mat.nu,
                props.base_mat.yield_strength,
                &props.yield_elem,
                hardening_n,
                fixed,
                force,
                name,
                false,
                &prescribed,
            )?
        } else {
            crate::run_case(
                mesh_pert,
                &Material::iso(props.young.clone(), props.base_mat.nu),
                fixed,
                force,
                name,
                false,
                &prescribed,
            )?
        };
        solves += 1;
        metrics.push(FixedCaseMetrics {
            compliance: case.compliance,
            max_displacement: case.max_displacement,
            max_vm_elem: case.max_vm_elem,
            max_temp: None,
        });
    }

    let thermal_cases = doc.thermal_cases();
    if !thermal_cases.is_empty() {
        let has_supports = fixed.iter().any(|&m| m != 0);
        let mut alphas = Vec::with_capacity(props.alpha_elem.len());
        if has_supports {
            for &a in &props.alpha_elem {
                let av = a.ok_or_else(|| {
                    SetupError(
                        "thermal case sensitivity requires a material thermal-expansion coefficient"
                            .into(),
                    )
                })?;
                alphas.push(av);
            }
        } else {
            alphas.resize(props.alpha_elem.len(), 0.0);
        }
        for name in thermal_cases {
            let case = run_thermal_case_multi_with_regions(
                doc,
                mesh_pert,
                &props.young,
                props.base_mat.nu,
                &alphas,
                fixed,
                facets,
                name,
                base_h,
                &props.k_elem,
                false,
                Some(thermal_lookup),
            )?;
            solves += 1;
            metrics.push(FixedCaseMetrics {
                compliance: case.compliance,
                max_displacement: case.max_displacement,
                max_vm_elem: case.max_vm_elem,
                max_temp: peak_temperature_opt(case.temperature.as_deref()),
            });
        }
    }
    if metrics.is_empty() && doc.solve.modes.is_some() {
        metrics.push(FixedCaseMetrics {
            compliance: 0.0,
            max_displacement: 0.0,
            max_vm_elem: 0.0,
            max_temp: None,
        });
    }

    Ok((metrics, solves))
}

/// Compute 3D P2 sensitivities for `params` (if non-empty) or all non-derived `let` / `system`
/// design / `fill.*` variables in `doc`, using fixed-topology boundary-velocity discrete
/// mechanical, staggered thermo-elastic, and graded TPMS lattice adjoints (`O(1)` linear solves in
/// parameter count `k`) whenever `solve plasticity=j2` is not active.
pub fn compute_sensitivities(
    doc: &Document,
    params: &[String],
    res: Resolution,
    clock: Clock,
) -> Result<SensitivityReport, SetupError> {
    let t0 = clock();
    let sys_doc_0 = resolve_system_doc(doc)?;
    let modes_count = required_modes_for_asserts(sys_doc_0.solve.modes, &doc.asserts);
    let base = analyse_full(sys_doc_0.as_ref(), res, clock, modes_count, false)?;
    if doc.fill.is_some() {
        let graded_0 = crate::fill::graded_of(sys_doc_0.as_ref(), &base, clock, false)?;
        return compute_sensitivities_graded_with_base(
            doc,
            sys_doc_0.as_ref(),
            &base,
            &graded_0,
            params,
            t0,
            clock,
        );
    }
    compute_sensitivities_with_base(doc, sys_doc_0.as_ref(), &base, params, t0, clock)
}

pub(crate) fn compute_sensitivities_with_base(
    doc: &Document,
    sys_doc_0_ref: &Document,
    base: &Analysis,
    params: &[String],
    t0: f64,
    clock: Clock,
) -> Result<SensitivityReport, SetupError> {
    let targets = default_target_params(doc, params);
    if doc.fill.is_some() {
        let graded_0 = crate::fill::graded_of(sys_doc_0_ref, base, clock, false)?;
        return compute_sensitivities_graded_with_base(
            doc,
            sys_doc_0_ref,
            base,
            &graded_0,
            params,
            t0,
            clock,
        );
    }

    let sys_doc_0 = std::borrow::Cow::Borrowed(sys_doc_0_ref);
    let sdoc_0_cow = structural_doc(sys_doc_0.as_ref());
    let sdoc_0 = sdoc_0_cow.as_ref();
    let base_h = base.provenance.element_size;
    let locked_elem_mats = lock_element_material_ids(sdoc_0, &base.mesh, base_h)?;
    let props_0 = eval_element_materials(sdoc_0, &locked_elem_mats)?;
    let base_mass = fixed_mesh_mass_grams(&base.mesh, &props_0.rho_elem);

    let facets: Vec<Facet> = base.surface.iter().map(|&nodes| Facet { nodes }).collect();

    // Lock baseline region -> facet index sets so fixed-topology perturbations are C^∞ smooth.
    let mut exact_region_map: HashMap<String, Vec<usize>> = HashMap::new();
    for l in &sdoc_0.loads {
        if !exact_region_map.contains_key(&l.region) {
            exact_region_map.insert(
                l.region.clone(),
                region_facets_exact(sdoc_0, &l.region, &base.mesh, &facets, base_h)?,
            );
        }
    }
    let mut thermal_region_map: HashMap<String, Vec<usize>> = HashMap::new();
    for reg_id in sdoc_0
        .temperatures
        .iter()
        .map(|t| &t.region)
        .chain(sdoc_0.convections.iter().map(|c| &c.region))
        .chain(sdoc_0.heat_fluxes.iter().map(|f| &f.region))
    {
        if !thermal_region_map.contains_key(reg_id) {
            thermal_region_map.insert(
                reg_id.clone(),
                region_facets(sdoc_0, reg_id, &base.mesh, &facets, base_h)?,
            );
        }
    }
    let exact_lookup = |rid: &str| -> Result<Vec<usize>, SetupError> {
        exact_region_map
            .get(rid)
            .cloned()
            .ok_or_else(|| SetupError(format!("unknown load region '{rid}'")))
    };
    let thermal_lookup = |rid: &str| -> Result<Vec<usize>, SetupError> {
        thermal_region_map
            .get(rid)
            .cloned()
            .ok_or_else(|| SetupError(format!("unknown thermal region '{rid}'")))
    };

    let nl_plastic = sdoc_0.solve.plasticity == Some(PlasticityLaw::J2);
    let thermal_cases = sdoc_0.thermal_cases();
    let mech_cases = case_names(sdoc_0);
    let use_adjoint = !nl_plastic;

    let mut base_alphas = Vec::new();
    if use_adjoint && !thermal_cases.is_empty() {
        let has_supports = base.fixed.iter().any(|&m| m != 0);
        if has_supports {
            for &a in &props_0.alpha_elem {
                base_alphas.push(a.ok_or_else(|| {
                    SetupError(
                        "thermal case sensitivity requires a material thermal-expansion coefficient"
                            .into(),
                    )
                })?);
            }
        } else {
            base_alphas.resize(props_0.alpha_elem.len(), 0.0);
        }
    }

    let base_static_solves = mech_cases.len() + thermal_cases.len();
    let base_modal_solves = usize::from(base.modes.is_some());
    let mut solves_run = (base_static_solves + base_modal_solves).max(1);
    let mut adjoints: Vec<CaseAdjoint> = Vec::new();
    if use_adjoint && !targets.is_empty() {
        for (idx, bc) in base.cases.iter().enumerate() {
            if idx < mech_cases.len() {
                let adj = build_linear_case_adjoint(
                    &base.mesh,
                    &base.young,
                    base.poisson,
                    &base.fixed,
                    &bc.u,
                    &bc.vm_elem,
                )?;
                solves_run += 2;
                adjoints.push(CaseAdjoint::Mechanical(adj));
            } else if idx < mech_cases.len() + thermal_cases.len() {
                let tname = &thermal_cases[idx - mech_cases.len()];
                let (tadj, extra_solves) = build_thermal_case_adjoint(
                    sdoc_0,
                    &base.mesh,
                    &facets,
                    base_h,
                    tname,
                    &props_0.k_elem,
                    &base.young,
                    base.poisson,
                    &base_alphas,
                    &base.fixed,
                    bc,
                    &thermal_lookup,
                )?;
                solves_run += extra_solves;
                adjoints.push(CaseAdjoint::Thermal(tadj));
            } else {
                adjoints.push(CaseAdjoint::ModalOnly);
            }
        }
    }

    let e0 = expr::compile_body(sdoc_0);
    let vert_adj = build_vertex_adjacency(&base.mesh);
    let rd = RefData::new();
    let th_rd = ThermalRefData::new();
    let mut out_params = Vec::with_capacity(targets.len());

    for pname in &targets {
        let addr = ParamAddr::parse(pname).map_err(SetupError)?;
        let p0 = read_param_value(doc, &addr).map_err(SetupError)?;
        let vel =
            compute_mesh_velocity(doc, &addr, p0, &e0, &base.mesh, &facets, &vert_adj, base_h)?;
        let v_max = vel
            .iter()
            .map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt())
            .fold(0.0f64, f64::max);

        let mut eps_p = (1e-5 * p0.abs()).max(1e-6);
        if v_max > 0.0 {
            eps_p = eps_p.min(1e-4 * base_h.max(1e-4) / v_max);
        }

        let mut d_plus = doc.clone();
        let mut d_minus = doc.clone();
        let ok_plus = apply_sens_param(&mut d_plus, &addr, p0 + eps_p);
        let ok_minus = apply_sens_param(&mut d_minus, &addr, p0 - eps_p);

        let (d_hi_raw, delta_hi, d_lo_raw, delta_lo, denom, scheme) = match (ok_plus, ok_minus) {
            (Ok(()), Ok(())) => (&d_plus, eps_p, &d_minus, -eps_p, 2.0 * eps_p, "central"),
            (Ok(()), Err(_)) => (&d_plus, eps_p, doc, 0.0, eps_p, "forward"),
            (Err(_), Ok(())) => (doc, 0.0, &d_minus, -eps_p, eps_p, "backward"),
            (Err(ep), Err(_)) => return Err(SetupError(ep)),
        };

        let (Some(mesh_hi), Some(mesh_lo)) = (
            deform_mesh_checked(&base.mesh, &vel, delta_hi),
            deform_mesh_checked(&base.mesh, &vel, delta_lo),
        ) else {
            return compute_sensitivities_remesh_with_base(doc, &targets, base, t0, clock);
        };

        let d_hi_sys = if scheme == "backward" {
            std::borrow::Cow::Borrowed(sys_doc_0.as_ref())
        } else {
            resolve_system_doc(d_hi_raw)?
        };
        let d_lo_sys = if scheme == "forward" {
            std::borrow::Cow::Borrowed(sys_doc_0.as_ref())
        } else {
            resolve_system_doc(d_lo_raw)?
        };

        let sdoc_hi = structural_doc(d_hi_sys.as_ref());
        let sdoc_lo = structural_doc(d_lo_sys.as_ref());
        let props_hi = eval_element_materials(sdoc_hi.as_ref(), &locked_elem_mats)?;
        let props_lo = eval_element_materials(sdoc_lo.as_ref(), &locked_elem_mats)?;
        let mat_hi = Material::iso(props_hi.young.clone(), props_hi.base_mat.nu);
        let mat_lo = Material::iso(props_lo.young.clone(), props_lo.base_mat.nu);

        let m_hi = fixed_mesh_mass_grams(&mesh_hi, &props_hi.rho_elem);
        let m_lo = fixed_mesh_mass_grams(&mesh_lo, &props_lo.rho_elem);
        let d_mass_g = (m_hi - m_lo) / denom;
        let rel_mass = rel_elasticity(p0, base_mass, d_mass_g);

        let mut cases = Vec::with_capacity(base.cases.len());
        if use_adjoint {
            for (idx, bc) in base.cases.iter().enumerate() {
                match &adjoints[idx] {
                    CaseAdjoint::Mechanical(adj) => {
                        let cname = &mech_cases[idx];
                        let (mut f_hi, _, _) = case_force_report_with_regions(
                            sdoc_hi.as_ref(),
                            &mesh_hi,
                            &facets,
                            cname,
                            base_h,
                            Some(&exact_lookup),
                        )?;
                        let (mut f_lo, _, _) = case_force_report_with_regions(
                            sdoc_lo.as_ref(),
                            &mesh_lo,
                            &facets,
                            cname,
                            base_h,
                            Some(&exact_lookup),
                        )?;
                        zero_fixed_dofs(&mut f_hi, &base.fixed);
                        zero_fixed_dofs(&mut f_lo, &base.fixed);
                        let df_dp: Vec<f64> = f_hi
                            .iter()
                            .zip(&f_lo)
                            .map(|(fh, fl)| (fh - fl) / denom)
                            .collect();

                        let dk_u = stiffness_matvec_derivative(
                            &mesh_hi,
                            &mat_hi,
                            &mesh_lo,
                            &mat_lo,
                            &bc.u,
                            &base.fixed,
                            denom,
                            &rd,
                        );
                        let r_k: Vec<f64> =
                            df_dp.iter().zip(&dk_u).map(|(df, dk)| df - dk).collect();

                        let d_compliance = dot_slice(&df_dp, &bc.u) + dot_slice(&bc.u, &r_k);
                        let d_max_displacement = dot_slice(&adj.lambda_u, &r_k);
                        let d_vm_explicit = if bc.max_vm_elem > 1e-30 && !base.mesh.tets.is_empty()
                        {
                            let e_star = adj.e_star;
                            let vm_hi = von_mises(&element_stress_mat(
                                &mesh_hi, &bc.u, e_star, &mat_hi, [0.25; 3],
                            ));
                            let vm_lo = von_mises(&element_stress_mat(
                                &mesh_lo, &bc.u, e_star, &mat_lo, [0.25; 3],
                            ));
                            (vm_hi - vm_lo) / denom
                        } else {
                            0.0
                        };
                        let d_max_vm_elem = d_vm_explicit + dot_slice(&adj.lambda_vm, &r_k);

                        cases.push(CaseSensitivity {
                            case: bc.name.clone(),
                            d_compliance,
                            rel_compliance: rel_elasticity(p0, bc.compliance, d_compliance),
                            d_max_displacement,
                            d_max_vm_elem,
                            d_max_temp: None,
                            d_yield_utilization: None,
                            d_buckling_utilization: None,
                        });
                    }
                    CaseAdjoint::Thermal(tadj) => {
                        let tname = &thermal_cases[idx - mech_cases.len()];
                        let t_0 = bc
                            .temperature
                            .as_deref()
                            .expect("thermal baseline temperature");
                        let (res_hi, t_lift_hi) = assemble_conduction_lifted_residual(
                            sdoc_hi.as_ref(),
                            &mesh_hi,
                            &facets,
                            base_h,
                            tname,
                            &props_hi.k_elem,
                            t_0,
                            &thermal_lookup,
                        )?;
                        let (res_lo, t_lift_lo) = assemble_conduction_lifted_residual(
                            sdoc_lo.as_ref(),
                            &mesh_lo,
                            &facets,
                            base_h,
                            tname,
                            &props_lo.k_elem,
                            t_0,
                            &thermal_lookup,
                        )?;
                        let r_t: Vec<f64> = res_hi
                            .iter()
                            .zip(&res_lo)
                            .map(|(rh, rl)| (rh - rl) / denom)
                            .collect();
                        let d_max_temp = (t_lift_hi[tadj.m_star] - t_lift_lo[tadj.m_star]) / denom
                            + dot_slice(&tadj.lambda_t_max, &r_t);

                        if tadj.pure_conduction {
                            cases.push(CaseSensitivity {
                                case: bc.name.clone(),
                                d_compliance: 0.0,
                                rel_compliance: 0.0,
                                d_max_displacement: 0.0,
                                d_max_vm_elem: 0.0,
                                d_max_temp: Some(d_max_temp),
                                d_yield_utilization: None,
                                d_buckling_utilization: None,
                            });
                        } else {
                            let alphas_hi: Vec<f64> = props_hi
                                .alpha_elem
                                .iter()
                                .map(|a| a.unwrap_or(0.0))
                                .collect();
                            let alphas_lo: Vec<f64> = props_lo
                                .alpha_elem
                                .iter()
                                .map(|a| a.unwrap_or(0.0))
                                .collect();
                            let tref_hi = sdoc_hi.solve.t_ref.unwrap_or(20.0);
                            let tref_lo = sdoc_lo.solve.t_ref.unwrap_or(20.0);
                            let dt_lift_hi: Vec<f64> =
                                t_lift_hi.iter().map(|&tv| tv - tref_hi).collect();
                            let dt_lift_lo: Vec<f64> =
                                t_lift_lo.iter().map(|&tv| tv - tref_lo).collect();

                            let mut f_th_hi = thermal_load_multi(
                                &mesh_hi,
                                &dt_lift_hi,
                                &props_hi.young,
                                props_hi.base_mat.nu,
                                &alphas_hi,
                                &th_rd,
                            );
                            let mut f_th_lo = thermal_load_multi(
                                &mesh_lo,
                                &dt_lift_lo,
                                &props_lo.young,
                                props_lo.base_mat.nu,
                                &alphas_lo,
                                &th_rd,
                            );
                            zero_fixed_dofs(&mut f_th_hi, &base.fixed);
                            zero_fixed_dofs(&mut f_th_lo, &base.fixed);
                            let df_th_explicit: Vec<f64> = f_th_hi
                                .iter()
                                .zip(&f_th_lo)
                                .map(|(fh, fl)| (fh - fl) / denom)
                                .collect();

                            let dk_u = stiffness_matvec_derivative(
                                &mesh_hi,
                                &mat_hi,
                                &mesh_lo,
                                &mat_lo,
                                &bc.u,
                                &base.fixed,
                                denom,
                                &rd,
                            );
                            let r_u_explicit: Vec<f64> = df_th_explicit
                                .iter()
                                .zip(&dk_u)
                                .map(|(df, dk)| df - dk)
                                .collect();

                            let d_compliance = dot_slice(&df_th_explicit, &bc.u)
                                + dot_slice(&bc.u, &r_u_explicit)
                                + dot_slice(&tadj.lambda_t_comp, &r_t);
                            let d_max_displacement = dot_slice(&tadj.lambda_u, &r_u_explicit)
                                + dot_slice(&tadj.lambda_t_u, &r_t);
                            let d_vm_explicit =
                                if bc.max_vm_elem > 1e-30 && !base.mesh.tets.is_empty() {
                                    let e_star = tadj.e_star;
                                    let vm_hi = von_mises(&element_stress_mat(
                                        &mesh_hi, &bc.u, e_star, &mat_hi, [0.25; 3],
                                    ));
                                    let vm_lo = von_mises(&element_stress_mat(
                                        &mesh_lo, &bc.u, e_star, &mat_lo, [0.25; 3],
                                    ));
                                    (vm_hi - vm_lo) / denom
                                } else {
                                    0.0
                                };
                            let d_max_vm_elem = d_vm_explicit
                                + dot_slice(&tadj.lambda_vm, &r_u_explicit)
                                + dot_slice(&tadj.lambda_t_vm, &r_t);

                            cases.push(CaseSensitivity {
                                case: bc.name.clone(),
                                d_compliance,
                                rel_compliance: rel_elasticity(p0, bc.compliance, d_compliance),
                                d_max_displacement,
                                d_max_vm_elem,
                                d_max_temp: Some(d_max_temp),
                                d_yield_utilization: None,
                                d_buckling_utilization: None,
                            });
                        }
                    }
                    CaseAdjoint::ModalOnly => {
                        cases.push(CaseSensitivity {
                            case: bc.name.clone(),
                            d_compliance: 0.0,
                            rel_compliance: 0.0,
                            d_max_displacement: 0.0,
                            d_max_vm_elem: 0.0,
                            d_max_temp: None,
                            d_yield_utilization: None,
                            d_buckling_utilization: None,
                        });
                    }
                }
            }
        } else {
            let (hi_metrics, s_hi) = if scheme == "backward" {
                (
                    base.cases
                        .iter()
                        .map(|c| FixedCaseMetrics {
                            compliance: c.compliance,
                            max_displacement: c.max_displacement,
                            max_vm_elem: c.max_vm_elem,
                            max_temp: peak_temperature_opt(c.temperature.as_deref()),
                        })
                        .collect(),
                    0,
                )
            } else {
                evaluate_cases_on_fixed_mesh(
                    d_hi_sys.as_ref(),
                    &mesh_hi,
                    &facets,
                    &base.fixed,
                    base_h,
                    &locked_elem_mats,
                    &exact_lookup,
                    &thermal_lookup,
                )?
            };
            let (lo_metrics, s_lo) = if scheme == "forward" {
                (
                    base.cases
                        .iter()
                        .map(|c| FixedCaseMetrics {
                            compliance: c.compliance,
                            max_displacement: c.max_displacement,
                            max_vm_elem: c.max_vm_elem,
                            max_temp: peak_temperature_opt(c.temperature.as_deref()),
                        })
                        .collect(),
                    0,
                )
            } else {
                evaluate_cases_on_fixed_mesh(
                    d_lo_sys.as_ref(),
                    &mesh_lo,
                    &facets,
                    &base.fixed,
                    base_h,
                    &locked_elem_mats,
                    &exact_lookup,
                    &thermal_lookup,
                )?
            };
            solves_run += s_hi + s_lo;
            for (idx, bc) in base.cases.iter().enumerate() {
                let hm = hi_metrics[idx];
                let lm = lo_metrics[idx];
                let d_compliance = (hm.compliance - lm.compliance) / denom;
                let d_max_temp = match (hm.max_temp, lm.max_temp) {
                    (Some(th), Some(tl)) => Some((th - tl) / denom),
                    _ => None,
                };
                cases.push(CaseSensitivity {
                    case: bc.name.clone(),
                    d_compliance,
                    rel_compliance: rel_elasticity(p0, bc.compliance, d_compliance),
                    d_max_displacement: (hm.max_displacement - lm.max_displacement) / denom,
                    d_max_vm_elem: (hm.max_vm_elem - lm.max_vm_elem) / denom,
                    d_max_temp,
                    d_yield_utilization: None,
                    d_buckling_utilization: None,
                });
            }
        }

        let (d_modes_hz, rel_modes) = if let Some(modal) = base.modes.as_ref() {
            modal_rayleigh_sensitivities(
                modal,
                p0,
                &mesh_hi,
                &props_hi,
                &mesh_lo,
                &props_lo,
                &base.fixed,
                denom,
                &rd,
            )
        } else {
            (Vec::new(), Vec::new())
        };

        out_params.push(ParameterSensitivity {
            param: pname.clone(),
            nominal: p0,
            step: eps_p,
            scheme: scheme.to_string(),
            d_mass_g,
            rel_mass,
            d_solid_mass_g: None,
            d_modes_hz,
            rel_modes,
            cases,
        });
    }

    let method = if use_adjoint {
        "boundary_velocity_adjoint"
    } else {
        "boundary_velocity_fixed_mesh"
    };
    let elapsed_ms = clock() - t0;
    Ok(SensitivityReport {
        method: method.to_string(),
        solves_run,
        h: base_h,
        nodes: base.provenance.nodes,
        elements: base.provenance.elements,
        baseline_mass_g: base_mass,
        baseline_solid_mass_g: None,
        baseline_compliance: base
            .cases
            .iter()
            .map(|c| (c.name.clone(), c.compliance))
            .collect(),
        baseline_modes_hz: base
            .modes
            .as_ref()
            .map(|m| m.frequencies.clone())
            .unwrap_or_default(),
        parameters: out_params,
        elapsed_ms,
    })
}

/// Fixed-topology boundary-velocity discrete adjoint sensitivity operator (`O(1)` linear solves
/// in parameter count `k`) for stress-graded TPMS `fill` models across both `fill.*` lattice
/// parameters and outer 3D geometry `let` / `system` design variables.
pub(crate) fn compute_sensitivities_graded_with_base(
    doc: &Document,
    sys_doc_0_ref: &Document,
    solid_base: &Analysis,
    graded_0: &crate::fill::Graded,
    params: &[String],
    t0: f64,
    clock: Clock,
) -> Result<SensitivityReport, SetupError> {
    let targets = default_target_params(doc, params);
    let sys_doc_0 = std::borrow::Cow::Borrowed(sys_doc_0_ref);
    let sdoc_0_cow = structural_doc(sys_doc_0.as_ref());
    let sdoc_0 = sdoc_0_cow.as_ref();
    let base_h = solid_base.provenance.element_size;
    let locked_elem_mats = lock_element_material_ids(sdoc_0, &solid_base.mesh, base_h)?;
    let elem_props_0 = eval_element_materials(sdoc_0, &locked_elem_mats)?;
    let smooth_0 = crate::fill::prepare_graded_smooth_baseline(
        sdoc_0,
        solid_base,
        graded_0,
        &elem_props_0.rho_elem,
    )?;
    let base_mass = graded_0.mass_g;
    let base_solid_mass = graded_0.solid_mass_g;

    let facets: Vec<Facet> = solid_base
        .surface
        .iter()
        .map(|&nodes| Facet { nodes })
        .collect();

    let mut exact_region_map: HashMap<String, Vec<usize>> = HashMap::new();
    for l in &sdoc_0.loads {
        if !exact_region_map.contains_key(&l.region) {
            exact_region_map.insert(
                l.region.clone(),
                region_facets_exact(sdoc_0, &l.region, &solid_base.mesh, &facets, base_h)?,
            );
        }
    }
    let mut thermal_region_map: HashMap<String, Vec<usize>> = HashMap::new();
    for reg_id in sdoc_0
        .temperatures
        .iter()
        .map(|t| &t.region)
        .chain(sdoc_0.convections.iter().map(|c| &c.region))
        .chain(sdoc_0.heat_fluxes.iter().map(|f| &f.region))
    {
        if !thermal_region_map.contains_key(reg_id) {
            thermal_region_map.insert(
                reg_id.clone(),
                region_facets(sdoc_0, reg_id, &solid_base.mesh, &facets, base_h)?,
            );
        }
    }
    let exact_lookup = |rid: &str| -> Result<Vec<usize>, SetupError> {
        exact_region_map
            .get(rid)
            .cloned()
            .ok_or_else(|| SetupError(format!("unknown load region '{rid}'")))
    };
    let thermal_lookup = |rid: &str| -> Result<Vec<usize>, SetupError> {
        thermal_region_map
            .get(rid)
            .cloned()
            .ok_or_else(|| SetupError(format!("unknown thermal region '{rid}'")))
    };

    let mech_cases = case_names(sdoc_0);
    let thermal_cases = sdoc_0.thermal_cases();
    let mut base_alphas = Vec::new();
    if !thermal_cases.is_empty() {
        let has_supports = solid_base.fixed.iter().any(|&m| m != 0);
        if has_supports {
            for &a in &elem_props_0.alpha_elem {
                base_alphas.push(a.ok_or_else(|| {
                    SetupError(
                        "thermal case sensitivity requires a material thermal-expansion coefficient"
                            .into(),
                    )
                })?);
            }
        } else {
            base_alphas.resize(elem_props_0.alpha_elem.len(), 0.0);
        }
    }

    let base_static_solves = mech_cases.len() + thermal_cases.len();
    let base_modal_solves = usize::from(solid_base.modes.is_some());
    let mut solves_run = (2 * base_static_solves + base_modal_solves).max(1);
    let mut adjoints: Vec<CaseAdjoint> = Vec::new();
    if !targets.is_empty() {
        for gc in &graded_0.cases {
            let gctx = GradedAdjointCtx {
                spec: &graded_0.spec,
                yield_strength: elem_props_0.base_mat.yield_strength,
                young_solid: elem_props_0.base_mat.e,
                class: &graded_0.class,
                rho_elem: &graded_0.rho_elem,
                sigma_min_elem: &gc.sigma_min_elem,
            };
            let (adj, adj_solves) = build_linear_case_adjoint_mat(
                &solid_base.mesh,
                &smooth_0.material_0,
                &solid_base.fixed,
                &gc.u,
                &gc.vm_elem,
                Some(&gctx),
            )?;
            solves_run += adj_solves;
            adjoints.push(CaseAdjoint::Mechanical(adj));
        }
        for (t_idx, gc) in graded_0.thermal_cases.iter().enumerate() {
            let tname = &thermal_cases[t_idx];
            let (tadj, extra_solves) = build_thermal_case_adjoint(
                sdoc_0,
                &solid_base.mesh,
                &facets,
                base_h,
                tname,
                &smooth_0.k_elem_0,
                &graded_0.young,
                elem_props_0.base_mat.nu,
                &base_alphas,
                &solid_base.fixed,
                gc,
                &thermal_lookup,
            )?;
            solves_run += extra_solves;
            adjoints.push(CaseAdjoint::Thermal(tadj));
        }
    }

    let e0 = expr::compile_body(sdoc_0);
    let vert_adj = build_vertex_adjacency(&solid_base.mesh);
    let rd = RefData::new();
    let th_rd = ThermalRefData::new();
    let mut out_params = Vec::with_capacity(targets.len());

    for pname in &targets {
        let addr = ParamAddr::parse(pname).map_err(SetupError)?;
        let p0 = read_param_value(doc, &addr).map_err(SetupError)?;
        let vel = if is_fill_param_target(doc, &addr) {
            vec![[0.0; 3]; solid_base.mesh.nodes.len()]
        } else {
            compute_mesh_velocity(
                doc,
                &addr,
                p0,
                &e0,
                &solid_base.mesh,
                &facets,
                &vert_adj,
                base_h,
            )?
        };
        let v_max = vel
            .iter()
            .map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt())
            .fold(0.0f64, f64::max);

        // `FillExpr.param` stores `f32` (`~1.2e-7` relative precision); `2e-3 * |p0|` stays
        // >10,000× above `f32` quantization while keeping `O(eps_p²)` truncation < 1e-5.
        let mut eps_p = (2e-3 * p0.abs()).max(5e-4);
        if v_max > 0.0 {
            eps_p = eps_p.min(0.02 * base_h.max(1e-4) / v_max);
        }

        let mut d_plus = doc.clone();
        let mut d_minus = doc.clone();
        let ok_plus = apply_sens_param(&mut d_plus, &addr, p0 + eps_p);
        let ok_minus = apply_sens_param(&mut d_minus, &addr, p0 - eps_p);

        let (d_hi_raw, delta_hi, d_lo_raw, delta_lo, denom, scheme) = match (ok_plus, ok_minus) {
            (Ok(()), Ok(())) => (&d_plus, eps_p, &d_minus, -eps_p, 2.0 * eps_p, "central"),
            (Ok(()), Err(_)) => (&d_plus, eps_p, doc, 0.0, eps_p, "forward"),
            (Err(_), Ok(())) => (doc, 0.0, &d_minus, -eps_p, eps_p, "backward"),
            (Err(ep), Err(_)) => return Err(SetupError(ep)),
        };

        let (Some(mesh_hi), Some(mesh_lo)) = (
            deform_mesh_checked(&solid_base.mesh, &vel, delta_hi),
            deform_mesh_checked(&solid_base.mesh, &vel, delta_lo),
        ) else {
            return compute_sensitivities_remesh_graded_with_base(
                doc, &targets, solid_base, graded_0, t0, clock,
            );
        };

        let d_hi_sys = if scheme == "backward" {
            std::borrow::Cow::Borrowed(sys_doc_0.as_ref())
        } else {
            resolve_system_doc(d_hi_raw)?
        };
        let d_lo_sys = if scheme == "forward" {
            std::borrow::Cow::Borrowed(sys_doc_0.as_ref())
        } else {
            resolve_system_doc(d_lo_raw)?
        };

        let sdoc_hi = structural_doc(d_hi_sys.as_ref());
        let sdoc_lo = structural_doc(d_lo_sys.as_ref());
        let elem_props_hi = eval_element_materials(sdoc_hi.as_ref(), &locked_elem_mats)?;
        let elem_props_lo = eval_element_materials(sdoc_lo.as_ref(), &locked_elem_mats)?;
        let gprops_hi = crate::fill::eval_graded_perturbed_props(
            sdoc_hi.as_ref(),
            &mesh_hi,
            base_h,
            graded_0,
            &smooth_0,
            &elem_props_hi.rho_elem,
        )?;
        let gprops_lo = crate::fill::eval_graded_perturbed_props(
            sdoc_lo.as_ref(),
            &mesh_lo,
            base_h,
            graded_0,
            &smooth_0,
            &elem_props_lo.rho_elem,
        )?;

        let d_mass_g = (gprops_hi.mass_g - gprops_lo.mass_g) / denom;
        let d_solid_mass_g = (gprops_hi.solid_mass_g - gprops_lo.solid_mass_g) / denom;
        let rel_mass = rel_elasticity(p0, base_mass, d_mass_g);

        let all_graded_cases: Vec<&CaseResult> = graded_0
            .cases
            .iter()
            .chain(graded_0.thermal_cases.iter())
            .collect();
        let mut cases = Vec::with_capacity(all_graded_cases.len());

        for (idx, gc) in all_graded_cases.iter().enumerate() {
            match &adjoints[idx] {
                CaseAdjoint::Mechanical(adj) => {
                    let cname = &mech_cases[idx];
                    let (mut f_hi, _, _) = case_force_report_with_regions(
                        sdoc_hi.as_ref(),
                        &mesh_hi,
                        &facets,
                        cname,
                        base_h,
                        Some(&exact_lookup),
                    )?;
                    let (mut f_lo, _, _) = case_force_report_with_regions(
                        sdoc_lo.as_ref(),
                        &mesh_lo,
                        &facets,
                        cname,
                        base_h,
                        Some(&exact_lookup),
                    )?;
                    zero_fixed_dofs(&mut f_hi, &solid_base.fixed);
                    zero_fixed_dofs(&mut f_lo, &solid_base.fixed);
                    let df_dp: Vec<f64> = f_hi
                        .iter()
                        .zip(&f_lo)
                        .map(|(fh, fl)| (fh - fl) / denom)
                        .collect();

                    let dk_u = stiffness_matvec_derivative(
                        &mesh_hi,
                        &gprops_hi.material,
                        &mesh_lo,
                        &gprops_lo.material,
                        &gc.u,
                        &solid_base.fixed,
                        denom,
                        &rd,
                    );
                    let r_k: Vec<f64> = df_dp.iter().zip(&dk_u).map(|(df, dk)| df - dk).collect();

                    let d_compliance = dot_slice(&df_dp, &gc.u) + dot_slice(&gc.u, &r_k);
                    let d_max_displacement = dot_slice(&adj.lambda_u, &r_k);
                    let d_vm_explicit =
                        if gc.max_vm_elem > 1e-30 && !solid_base.mesh.tets.is_empty() {
                            let e_star = adj.e_star;
                            let vm_hi = von_mises(&element_stress_mat(
                                &mesh_hi,
                                &gc.u,
                                e_star,
                                &gprops_hi.material,
                                [0.25; 3],
                            ));
                            let vm_lo = von_mises(&element_stress_mat(
                                &mesh_lo,
                                &gc.u,
                                e_star,
                                &gprops_lo.material,
                                [0.25; 3],
                            ));
                            (vm_hi - vm_lo) / denom
                        } else {
                            0.0
                        };
                    let d_max_vm_elem = d_vm_explicit + dot_slice(&adj.lambda_vm, &r_k);

                    let d_yield_utilization = if let Some((ey, ref lambda_uy)) = adj.yield_util {
                        let sy_hi = crate::fill::strength_extended(
                            gprops_hi.spec.family,
                            gprops_hi.spec.mode,
                            gprops_hi.base_mat.yield_strength,
                            gprops_hi.phi_elem[ey],
                        );
                        let sy_lo = crate::fill::strength_extended(
                            gprops_lo.spec.family,
                            gprops_lo.spec.mode,
                            gprops_lo.base_mat.yield_strength,
                            gprops_lo.phi_elem[ey],
                        );
                        let uy_hi = if sy_hi > 1e-30 {
                            von_mises(&element_stress_mat(
                                &mesh_hi,
                                &gc.u,
                                ey,
                                &gprops_hi.material,
                                [0.25; 3],
                            )) / sy_hi
                        } else {
                            0.0
                        };
                        let uy_lo = if sy_lo > 1e-30 {
                            von_mises(&element_stress_mat(
                                &mesh_lo,
                                &gc.u,
                                ey,
                                &gprops_lo.material,
                                [0.25; 3],
                            )) / sy_lo
                        } else {
                            0.0
                        };
                        Some((uy_hi - uy_lo) / denom + dot_slice(lambda_uy, &r_k))
                    } else {
                        graded_0
                            .yield_utilization
                            .get(idx)
                            .and_then(|v| *v)
                            .map(|_| 0.0)
                    };

                    let d_buckling_utilization =
                        if let Some((eb, ref lambda_ub)) = adj.buckling_util {
                            let sb_hi = crate::fill::buckling_extended(
                                gprops_hi.spec.family,
                                gprops_hi.spec.mode,
                                gprops_hi.base_mat.e,
                                gprops_hi.phi_elem[eb],
                            );
                            let sb_lo = crate::fill::buckling_extended(
                                gprops_lo.spec.family,
                                gprops_lo.spec.mode,
                                gprops_lo.base_mat.e,
                                gprops_lo.phi_elem[eb],
                            );
                            let sc_hi = -min_compressive(&element_stress_mat(
                                &mesh_hi,
                                &gc.u,
                                eb,
                                &gprops_hi.material,
                                [0.25; 3],
                            ));
                            let sc_lo = -min_compressive(&element_stress_mat(
                                &mesh_lo,
                                &gc.u,
                                eb,
                                &gprops_lo.material,
                                [0.25; 3],
                            ));
                            let ub_hi = if sb_hi > 1e-30 { sc_hi / sb_hi } else { 0.0 };
                            let ub_lo = if sb_lo > 1e-30 { sc_lo / sb_lo } else { 0.0 };
                            Some((ub_hi - ub_lo) / denom + dot_slice(lambda_ub, &r_k))
                        } else {
                            Some(0.0)
                        };

                    cases.push(CaseSensitivity {
                        case: gc.name.clone(),
                        d_compliance,
                        rel_compliance: rel_elasticity(p0, gc.compliance, d_compliance),
                        d_max_displacement,
                        d_max_vm_elem,
                        d_max_temp: None,
                        d_yield_utilization,
                        d_buckling_utilization,
                    });
                }
                CaseAdjoint::Thermal(tadj) => {
                    let tname = &thermal_cases[idx - mech_cases.len()];
                    let t_0 = gc
                        .temperature
                        .as_deref()
                        .expect("graded thermal baseline temperature");
                    let (res_hi, t_lift_hi) = assemble_conduction_lifted_residual(
                        sdoc_hi.as_ref(),
                        &mesh_hi,
                        &facets,
                        base_h,
                        tname,
                        &gprops_hi.k_elem,
                        t_0,
                        &thermal_lookup,
                    )?;
                    let (res_lo, t_lift_lo) = assemble_conduction_lifted_residual(
                        sdoc_lo.as_ref(),
                        &mesh_lo,
                        &facets,
                        base_h,
                        tname,
                        &gprops_lo.k_elem,
                        t_0,
                        &thermal_lookup,
                    )?;
                    let r_t: Vec<f64> = res_hi
                        .iter()
                        .zip(&res_lo)
                        .map(|(rh, rl)| (rh - rl) / denom)
                        .collect();
                    let d_max_temp = (t_lift_hi[tadj.m_star] - t_lift_lo[tadj.m_star]) / denom
                        + dot_slice(&tadj.lambda_t_max, &r_t);

                    if tadj.pure_conduction {
                        cases.push(CaseSensitivity {
                            case: gc.name.clone(),
                            d_compliance: 0.0,
                            rel_compliance: 0.0,
                            d_max_displacement: 0.0,
                            d_max_vm_elem: 0.0,
                            d_max_temp: Some(d_max_temp),
                            d_yield_utilization: None,
                            d_buckling_utilization: None,
                        });
                    } else {
                        let alphas_hi: Vec<f64> = elem_props_hi
                            .alpha_elem
                            .iter()
                            .map(|a| a.unwrap_or(0.0))
                            .collect();
                        let alphas_lo: Vec<f64> = elem_props_lo
                            .alpha_elem
                            .iter()
                            .map(|a| a.unwrap_or(0.0))
                            .collect();
                        let tref_hi = sdoc_hi.solve.t_ref.unwrap_or(20.0);
                        let tref_lo = sdoc_lo.solve.t_ref.unwrap_or(20.0);
                        let dt_lift_hi: Vec<f64> =
                            t_lift_hi.iter().map(|&tv| tv - tref_hi).collect();
                        let dt_lift_lo: Vec<f64> =
                            t_lift_lo.iter().map(|&tv| tv - tref_lo).collect();

                        let iso_hi =
                            Material::iso(gprops_hi.material.young.clone(), gprops_hi.base_mat.nu);
                        let iso_lo =
                            Material::iso(gprops_lo.material.young.clone(), gprops_lo.base_mat.nu);
                        let mut f_th_hi = thermal_load_multi(
                            &mesh_hi,
                            &dt_lift_hi,
                            &iso_hi.young,
                            iso_hi.poisson,
                            &alphas_hi,
                            &th_rd,
                        );
                        let mut f_th_lo = thermal_load_multi(
                            &mesh_lo,
                            &dt_lift_lo,
                            &iso_lo.young,
                            iso_lo.poisson,
                            &alphas_lo,
                            &th_rd,
                        );
                        zero_fixed_dofs(&mut f_th_hi, &solid_base.fixed);
                        zero_fixed_dofs(&mut f_th_lo, &solid_base.fixed);
                        let df_th_explicit: Vec<f64> = f_th_hi
                            .iter()
                            .zip(&f_th_lo)
                            .map(|(fh, fl)| (fh - fl) / denom)
                            .collect();

                        let dk_u = stiffness_matvec_derivative(
                            &mesh_hi,
                            &iso_hi,
                            &mesh_lo,
                            &iso_lo,
                            &gc.u,
                            &solid_base.fixed,
                            denom,
                            &rd,
                        );
                        let r_u_explicit: Vec<f64> = df_th_explicit
                            .iter()
                            .zip(&dk_u)
                            .map(|(df, dk)| df - dk)
                            .collect();

                        let d_compliance = dot_slice(&df_th_explicit, &gc.u)
                            + dot_slice(&gc.u, &r_u_explicit)
                            + dot_slice(&tadj.lambda_t_comp, &r_t);
                        let d_max_displacement = dot_slice(&tadj.lambda_u, &r_u_explicit)
                            + dot_slice(&tadj.lambda_t_u, &r_t);
                        let d_vm_explicit =
                            if gc.max_vm_elem > 1e-30 && !solid_base.mesh.tets.is_empty() {
                                let e_star = tadj.e_star;
                                let vm_hi = von_mises(&element_stress_mat(
                                    &mesh_hi, &gc.u, e_star, &iso_hi, [0.25; 3],
                                ));
                                let vm_lo = von_mises(&element_stress_mat(
                                    &mesh_lo, &gc.u, e_star, &iso_lo, [0.25; 3],
                                ));
                                (vm_hi - vm_lo) / denom
                            } else {
                                0.0
                            };
                        let d_max_vm_elem = d_vm_explicit
                            + dot_slice(&tadj.lambda_vm, &r_u_explicit)
                            + dot_slice(&tadj.lambda_t_vm, &r_t);

                        cases.push(CaseSensitivity {
                            case: gc.name.clone(),
                            d_compliance,
                            rel_compliance: rel_elasticity(p0, gc.compliance, d_compliance),
                            d_max_displacement,
                            d_max_vm_elem,
                            d_max_temp: Some(d_max_temp),
                            d_yield_utilization: None,
                            d_buckling_utilization: None,
                        });
                    }
                }
                CaseAdjoint::ModalOnly => {}
            }
        }

        let (d_modes_hz, rel_modes) = if let Some(modal) = solid_base.modes.as_ref() {
            modal_rayleigh_sensitivities(
                modal,
                p0,
                &mesh_hi,
                &elem_props_hi,
                &mesh_lo,
                &elem_props_lo,
                &solid_base.fixed,
                denom,
                &rd,
            )
        } else {
            (Vec::new(), Vec::new())
        };

        out_params.push(ParameterSensitivity {
            param: pname.clone(),
            nominal: p0,
            step: eps_p,
            scheme: scheme.to_string(),
            d_mass_g,
            rel_mass,
            d_solid_mass_g: Some(d_solid_mass_g),
            d_modes_hz,
            rel_modes,
            cases,
        });
    }

    let elapsed_ms = clock() - t0;
    Ok(SensitivityReport {
        method: "boundary_velocity_adjoint".to_string(),
        solves_run,
        h: base_h,
        nodes: solid_base.provenance.nodes,
        elements: solid_base.provenance.elements,
        baseline_mass_g: base_mass,
        baseline_solid_mass_g: Some(base_solid_mass),
        baseline_compliance: graded_0
            .cases
            .iter()
            .chain(graded_0.thermal_cases.iter())
            .map(|c| (c.name.clone(), c.compliance))
            .collect(),
        baseline_modes_hz: solid_base
            .modes
            .as_ref()
            .map(|m| m.frequencies.clone())
            .unwrap_or_default(),
        parameters: out_params,
        elapsed_ms,
    })
}

/// Locked-`h` re-meshing finite-difference sensitivity operator (`2k + 1` re-stuffed tetrahedral
/// meshes and solves). Retained as fallback when a degenerate perturbation cannot preserve
/// positive element volumes on the baseline topology, and as an independent cross-verification
/// reference for [`compute_sensitivities`].
pub fn compute_sensitivities_remesh(
    doc: &Document,
    params: &[String],
    res: Resolution,
    clock: Clock,
) -> Result<SensitivityReport, SetupError> {
    let t0 = clock();
    let targets = default_target_params(doc, params);
    let sys_doc_0 = resolve_system_doc(doc)?;
    let modes_count = required_modes_for_asserts(sys_doc_0.solve.modes, &doc.asserts);
    let base = analyse_full(sys_doc_0.as_ref(), res, clock, modes_count, false)?;
    if doc.fill.is_some() {
        let graded_0 = crate::fill::graded_of(sys_doc_0.as_ref(), &base, clock, false)?;
        return compute_sensitivities_remesh_graded_with_base(
            doc, &targets, &base, &graded_0, t0, clock,
        );
    }
    compute_sensitivities_remesh_with_base(doc, &targets, &base, t0, clock)
}

fn compute_sensitivities_remesh_graded_with_base(
    doc: &Document,
    targets: &[String],
    solid_base: &Analysis,
    graded_0: &crate::fill::Graded,
    t0: f64,
    clock: Clock,
) -> Result<SensitivityReport, SetupError> {
    let sys_doc_0 = resolve_system_doc(doc)?;
    let base_h = solid_base.provenance.element_size;
    let locked_res = Resolution::Size(base_h);
    let modes_count = solid_base
        .modes
        .as_ref()
        .map(|m| m.frequencies.len())
        .or_else(|| required_modes_for_asserts(sys_doc_0.solve.modes, &doc.asserts));

    let mut solves_run = 2usize;
    let mut out_params = Vec::with_capacity(targets.len());
    for pname in targets {
        let addr = ParamAddr::parse(pname).map_err(SetupError)?;
        let p0 = read_param_value(doc, &addr).map_err(SetupError)?;
        let delta = (2e-2 * p0.abs()).max(5e-3);

        let eval_at = |val: f64| -> Result<(Analysis, crate::fill::Graded), SetupError> {
            let mut d_pert = doc.clone();
            apply_sens_param(&mut d_pert, &addr, val).map_err(SetupError)?;
            let r_pert = resolve_system_doc(&d_pert)?;
            let a = analyse_full(r_pert.as_ref(), locked_res, clock, modes_count, false)?;
            let g = crate::fill::graded_of(r_pert.as_ref(), &a, clock, false)?;
            Ok((a, g))
        };

        let plus = eval_at(p0 + delta);
        let minus = eval_at(p0 - delta);

        let (a_hi_ref, g_hi_ref, a_lo_ref, g_lo_ref, denom, scheme): (
            &Analysis,
            &crate::fill::Graded,
            &Analysis,
            &crate::fill::Graded,
            f64,
            &str,
        );
        let ap_holder;
        let gp_holder;
        let am_holder;
        let gm_holder;
        match (plus, minus) {
            (Ok((ap, gp)), Ok((am, gm))) => {
                solves_run += 4;
                ap_holder = ap;
                gp_holder = gp;
                am_holder = am;
                gm_holder = gm;
                a_hi_ref = &ap_holder;
                g_hi_ref = &gp_holder;
                a_lo_ref = &am_holder;
                g_lo_ref = &gm_holder;
                denom = 2.0 * delta;
                scheme = "central";
            }
            (Ok((ap, gp)), Err(_)) => {
                solves_run += 2;
                ap_holder = ap;
                gp_holder = gp;
                a_hi_ref = &ap_holder;
                g_hi_ref = &gp_holder;
                a_lo_ref = solid_base;
                g_lo_ref = graded_0;
                denom = delta;
                scheme = "forward";
            }
            (Err(_), Ok((am, gm))) => {
                solves_run += 2;
                am_holder = am;
                gm_holder = gm;
                a_hi_ref = solid_base;
                g_hi_ref = graded_0;
                a_lo_ref = &am_holder;
                g_lo_ref = &gm_holder;
                denom = delta;
                scheme = "backward";
            }
            (Err(ep), Err(_)) => return Err(ep),
        }

        let d_mass_g = (g_hi_ref.mass_g - g_lo_ref.mass_g) / denom;
        let d_solid_mass_g = (g_hi_ref.solid_mass_g - g_lo_ref.solid_mass_g) / denom;
        let rel_mass = rel_elasticity(p0, graded_0.mass_g, d_mass_g);

        let mut d_modes_hz = Vec::new();
        let mut rel_modes = Vec::new();
        if let (Some(bm), Some(mh), Some(ml)) = (
            solid_base.modes.as_ref(),
            a_hi_ref.modes.as_ref(),
            a_lo_ref.modes.as_ref(),
        ) {
            let nm = bm
                .frequencies
                .len()
                .min(mh.frequencies.len())
                .min(ml.frequencies.len());
            for m_idx in 0..nm {
                let df = (mh.frequencies[m_idx] - ml.frequencies[m_idx]) / denom;
                d_modes_hz.push(df);
                rel_modes.push(rel_elasticity(p0, bm.frequencies[m_idx], df));
            }
        }

        let base_all: Vec<&CaseResult> = graded_0
            .cases
            .iter()
            .chain(graded_0.thermal_cases.iter())
            .collect();
        let hi_all: Vec<&CaseResult> = g_hi_ref
            .cases
            .iter()
            .chain(g_hi_ref.thermal_cases.iter())
            .collect();
        let lo_all: Vec<&CaseResult> = g_lo_ref
            .cases
            .iter()
            .chain(g_lo_ref.thermal_cases.iter())
            .collect();
        let n_mech = graded_0.cases.len();
        let mut cases = Vec::with_capacity(base_all.len());
        for (idx, gc) in base_all.iter().enumerate() {
            let Some(ch) = hi_all.get(idx) else {
                continue;
            };
            let Some(cl) = lo_all.get(idx) else {
                continue;
            };
            let d_compliance = (ch.compliance - cl.compliance) / denom;
            let d_max_displacement = (ch.max_displacement - cl.max_displacement) / denom;
            let d_max_vm_elem = (ch.max_vm_elem - cl.max_vm_elem) / denom;
            let d_max_temp = match (
                peak_temperature_opt(ch.temperature.as_deref()),
                peak_temperature_opt(cl.temperature.as_deref()),
            ) {
                (Some(th), Some(tl)) => Some((th - tl) / denom),
                _ => None,
            };
            let d_yield_utilization = if idx < n_mech {
                match (
                    g_hi_ref.yield_utilization.get(idx).and_then(|v| *v),
                    g_lo_ref.yield_utilization.get(idx).and_then(|v| *v),
                ) {
                    (Some(uh), Some(ul)) => Some((uh - ul) / denom),
                    _ => None,
                }
            } else {
                None
            };
            let d_buckling_utilization = if idx < n_mech {
                match (
                    g_hi_ref.buckling_utilization.get(idx),
                    g_lo_ref.buckling_utilization.get(idx),
                ) {
                    (Some(bh), Some(bl)) => Some((bh - bl) / denom),
                    _ => None,
                }
            } else {
                None
            };
            cases.push(CaseSensitivity {
                case: gc.name.clone(),
                d_compliance,
                rel_compliance: rel_elasticity(p0, gc.compliance, d_compliance),
                d_max_displacement,
                d_max_vm_elem,
                d_max_temp,
                d_yield_utilization,
                d_buckling_utilization,
            });
        }

        out_params.push(ParameterSensitivity {
            param: pname.clone(),
            nominal: p0,
            step: delta,
            scheme: scheme.to_string(),
            d_mass_g,
            rel_mass,
            d_solid_mass_g: Some(d_solid_mass_g),
            d_modes_hz,
            rel_modes,
            cases,
        });
    }

    let elapsed_ms = clock() - t0;
    Ok(SensitivityReport {
        method: "finite_difference_remesh".to_string(),
        solves_run,
        h: base_h,
        nodes: solid_base.provenance.nodes,
        elements: solid_base.provenance.elements,
        baseline_mass_g: graded_0.mass_g,
        baseline_solid_mass_g: Some(graded_0.solid_mass_g),
        baseline_compliance: graded_0
            .cases
            .iter()
            .chain(graded_0.thermal_cases.iter())
            .map(|c| (c.name.clone(), c.compliance))
            .collect(),
        baseline_modes_hz: solid_base
            .modes
            .as_ref()
            .map(|m| m.frequencies.clone())
            .unwrap_or_default(),
        parameters: out_params,
        elapsed_ms,
    })
}

fn compute_sensitivities_remesh_with_base(
    doc: &Document,
    targets: &[String],
    base: &Analysis,
    t0: f64,
    clock: Clock,
) -> Result<SensitivityReport, SetupError> {
    let sys_doc_0 = resolve_system_doc(doc)?;
    let base_mass = mesh_mass_grams(sys_doc_0.as_ref(), base);
    let base_h = base.provenance.element_size;
    let locked_res = Resolution::Size(base_h);
    let modes_count = base
        .modes
        .as_ref()
        .map(|m| m.frequencies.len())
        .or_else(|| required_modes_for_asserts(sys_doc_0.solve.modes, &doc.asserts));

    let mut solves_run = 1usize;
    let mut out_params = Vec::with_capacity(targets.len());
    for pname in targets {
        let addr = ParamAddr::parse(pname).map_err(SetupError)?;
        let p0 = read_param_value(doc, &addr).map_err(SetupError)?;
        let delta = (5e-3 * p0.abs()).max(1e-3);

        let eval_at = |val: f64| -> Result<(Analysis, f64), SetupError> {
            let mut d_pert = doc.clone();
            apply_sens_param(&mut d_pert, &addr, val).map_err(SetupError)?;
            let r_pert = resolve_system_doc(&d_pert)?;
            let a = analyse_full(r_pert.as_ref(), locked_res, clock, modes_count, false)?;
            let m = mesh_mass_grams(r_pert.as_ref(), &a);
            Ok((a, m))
        };

        let plus = eval_at(p0 + delta);
        let minus = eval_at(p0 - delta);

        let (a_hi_ref, m_hi, a_lo_ref, m_lo, denom, scheme): (
            &Analysis,
            f64,
            &Analysis,
            f64,
            f64,
            &str,
        );
        let ap_holder;
        let am_holder;
        match (plus, minus) {
            (Ok((ap, mp)), Ok((am, mm))) => {
                solves_run += 2;
                ap_holder = ap;
                am_holder = am;
                a_hi_ref = &ap_holder;
                m_hi = mp;
                a_lo_ref = &am_holder;
                m_lo = mm;
                denom = 2.0 * delta;
                scheme = "central";
            }
            (Ok((ap, mp)), Err(_)) => {
                solves_run += 1;
                ap_holder = ap;
                a_hi_ref = &ap_holder;
                m_hi = mp;
                a_lo_ref = base;
                m_lo = base_mass;
                denom = delta;
                scheme = "forward";
            }
            (Err(_), Ok((am, mm))) => {
                solves_run += 1;
                am_holder = am;
                a_hi_ref = base;
                m_hi = base_mass;
                a_lo_ref = &am_holder;
                m_lo = mm;
                denom = delta;
                scheme = "backward";
            }
            (Err(ep), Err(_)) => return Err(ep),
        }

        let d_mass_g = (m_hi - m_lo) / denom;
        let rel_mass = rel_elasticity(p0, base_mass, d_mass_g);

        let mut d_modes_hz = Vec::new();
        let mut rel_modes = Vec::new();
        if let (Some(bm), Some(mh), Some(ml)) = (
            base.modes.as_ref(),
            a_hi_ref.modes.as_ref(),
            a_lo_ref.modes.as_ref(),
        ) {
            let nm = bm
                .frequencies
                .len()
                .min(mh.frequencies.len())
                .min(ml.frequencies.len());
            for m_idx in 0..nm {
                let df = (mh.frequencies[m_idx] - ml.frequencies[m_idx]) / denom;
                d_modes_hz.push(df);
                rel_modes.push(rel_elasticity(p0, bm.frequencies[m_idx], df));
            }
        }

        let mut cases = Vec::with_capacity(base.cases.len());
        for (idx, bc) in base.cases.iter().enumerate() {
            let Some(ch) = a_hi_ref.cases.get(idx) else {
                continue;
            };
            let Some(cl) = a_lo_ref.cases.get(idx) else {
                continue;
            };
            let d_compliance = (ch.compliance - cl.compliance) / denom;
            let d_max_displacement = (ch.max_displacement - cl.max_displacement) / denom;
            let d_max_vm_elem = (ch.max_vm_elem - cl.max_vm_elem) / denom;
            let d_max_temp = match (
                peak_temperature_opt(ch.temperature.as_deref()),
                peak_temperature_opt(cl.temperature.as_deref()),
            ) {
                (Some(th), Some(tl)) => Some((th - tl) / denom),
                _ => None,
            };
            cases.push(CaseSensitivity {
                case: bc.name.clone(),
                d_compliance,
                rel_compliance: rel_elasticity(p0, bc.compliance, d_compliance),
                d_max_displacement,
                d_max_vm_elem,
                d_max_temp,
                d_yield_utilization: None,
                d_buckling_utilization: None,
            });
        }

        out_params.push(ParameterSensitivity {
            param: pname.clone(),
            nominal: p0,
            step: delta,
            scheme: scheme.to_string(),
            d_mass_g,
            rel_mass,
            d_solid_mass_g: None,
            d_modes_hz,
            rel_modes,
            cases,
        });
    }

    let elapsed_ms = clock() - t0;
    Ok(SensitivityReport {
        method: "finite_difference_remesh".to_string(),
        solves_run,
        h: base_h,
        nodes: base.provenance.nodes,
        elements: base.provenance.elements,
        baseline_mass_g: base_mass,
        baseline_solid_mass_g: None,
        baseline_compliance: base
            .cases
            .iter()
            .map(|c| (c.name.clone(), c.compliance))
            .collect(),
        baseline_modes_hz: base
            .modes
            .as_ref()
            .map(|m| m.frequencies.clone())
            .unwrap_or_default(),
        parameters: out_params,
        elapsed_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyse;
    use fieldsmith_doc::fsm;

    fn zero_clock() -> f64 {
        0.0
    }

    #[test]
    fn cantilever_adjoint_sensitivities_scale_o1_in_params_and_match_beam_laws() {
        let src = "\
fieldsmith 1
name \"Slender Sensitivity Cantilever\"
custommaterial steel200 e=200000 nu=0.30 yield=250 density=7.85
material steel200
let span = 80
let width = 10
let depth = 8
let tip_f = -400
box beam size=(span, width, depth) at=(span / 2, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, tip_f)
";
        let doc = fsm::parse(src).expect("parse");
        let rep = compute_sensitivities(&doc, &[], Resolution::Dofs(15_000), zero_clock)
            .expect("adjoint sensitivities");
        assert_eq!(rep.method, "boundary_velocity_adjoint");
        // 4 independent parameters (`span`, `width`, `depth`, `tip_f`), yet only 3 linear solves
        // total (1 forward + 2 adjoints reusing 1 preconditioner), NOT 2*4 + 1 = 9!
        assert_eq!(rep.parameters.len(), 4);
        assert_eq!(rep.solves_run, 3);

        let sl = rep.parameters.iter().find(|p| p.param == "span").unwrap();
        let sw = rep.parameters.iter().find(|p| p.param == "width").unwrap();
        let sh = rep.parameters.iter().find(|p| p.param == "depth").unwrap();
        let sf = rep.parameters.iter().find(|p| p.param == "tip_f").unwrap();

        // Exact box mass elasticities: m = ρ * span * width * depth => +1.0 for each dimension, 0 for force
        assert!(
            (sl.rel_mass - 1.0).abs() < 1e-4,
            "expected span mass elasticity +1.0000, got {}",
            sl.rel_mass
        );
        assert!(
            (sw.rel_mass - 1.0).abs() < 1e-4,
            "expected width mass elasticity +1.0000, got {}",
            sw.rel_mass
        );
        assert!(
            (sh.rel_mass - 1.0).abs() < 1e-4,
            "expected depth mass elasticity +1.0000, got {}",
            sh.rel_mass
        );
        assert!(sf.d_mass_g.abs() < 1e-12);

        // Quadratic compliance scaling in applied load: C ∝ F^2 => (F/C)(∂C/∂F) = +2.000000,
        // and linear displacement / von Mises scaling in F => +1.000000!
        let c0 = rep.baseline_compliance[0].1;
        assert!(
            (sf.cases[0].rel_compliance - 2.0).abs() < 1e-6,
            "expected force compliance elasticity +2.0, got {}",
            sf.cases[0].rel_compliance
        );
        assert!(c0 > 0.0);

        // 3D Timoshenko beam compliance elasticities: width ≈ -1.0, depth ≈ -2.93, span ≈ +2.93
        let rel_cl = sl.cases[0].rel_compliance;
        let rel_cw = sw.cases[0].rel_compliance;
        let rel_ch = sh.cases[0].rel_compliance;
        assert!(
            (rel_cw - (-1.0)).abs() < 0.05,
            "expected width compliance elasticity ~ -1.0, got {rel_cw}"
        );
        assert!(
            (rel_ch - (-2.93)).abs() < 0.10,
            "expected depth compliance elasticity ~ -2.93, got {rel_ch}"
        );
        assert!(
            (rel_cl - 2.93).abs() < 0.10,
            "expected span compliance elasticity ~ +2.93, got {rel_cl}"
        );

        // Cross-check ALL 3 observables (`d_compliance`, `d_max_displacement`, `d_max_vm_elem`)
        // of discrete adjoints against direct fixed-mesh state solves (`evaluate_cases_on_fixed_mesh`)
        // to verify exact discrete adjoint consistency (`< 1e-5` relative error)!
        let base = analyse(&doc, Resolution::Size(rep.h), zero_clock).unwrap();
        let sdoc_0 = structural_doc(&doc);
        let locked_mats = lock_element_material_ids(&sdoc_0, &base.mesh, rep.h).unwrap();
        let facets: Vec<Facet> = base.surface.iter().map(|&nodes| Facet { nodes }).collect();
        let mut exact_map = HashMap::new();
        for l in &sdoc_0.loads {
            exact_map.insert(
                l.region.clone(),
                region_facets_exact(&sdoc_0, &l.region, &base.mesh, &facets, rep.h).unwrap(),
            );
        }
        let exact_lookup = |rid: &str| Ok(exact_map[rid].clone());
        let thermal_lookup = |_rid: &str| Ok(Vec::new());
        let e0 = expr::compile_body(&sdoc_0);
        let vert_adj = build_vertex_adjacency(&base.mesh);

        for ps in [&sl, &sw, &sh, &sf] {
            let addr = ParamAddr::parse(&ps.param).unwrap();
            let vel = compute_mesh_velocity(
                &doc, &addr, ps.nominal, &e0, &base.mesh, &facets, &vert_adj, rep.h,
            )
            .unwrap();
            let eps = ps.step;
            let mut d_hi = doc.clone();
            let mut d_lo = doc.clone();
            param::apply(&mut d_hi, &addr, ps.nominal + eps).unwrap();
            param::apply(&mut d_lo, &addr, ps.nominal - eps).unwrap();
            let m_hi = deform_mesh_checked(&base.mesh, &vel, eps).unwrap();
            let m_lo = deform_mesh_checked(&base.mesh, &vel, -eps).unwrap();
            let (hi_m, _) = evaluate_cases_on_fixed_mesh(
                &d_hi,
                &m_hi,
                &facets,
                &base.fixed,
                rep.h,
                &locked_mats,
                &exact_lookup,
                &thermal_lookup,
            )
            .unwrap();
            let (lo_m, _) = evaluate_cases_on_fixed_mesh(
                &d_lo,
                &m_lo,
                &facets,
                &base.fixed,
                rep.h,
                &locked_mats,
                &exact_lookup,
                &thermal_lookup,
            )
            .unwrap();
            let fd_comp = (hi_m[0].compliance - lo_m[0].compliance) / (2.0 * eps);
            let fd_u = (hi_m[0].max_displacement - lo_m[0].max_displacement) / (2.0 * eps);
            let fd_vm = (hi_m[0].max_vm_elem - lo_m[0].max_vm_elem) / (2.0 * eps);
            let cs = &ps.cases[0];

            let rel_err = |a: f64, b: f64| (a - b).abs() / b.abs().max(1e-9);
            assert!(
                rel_err(cs.d_compliance, fd_comp) < 1e-5,
                "param {} d_compliance adjoint={} vs fixed-mesh FD={}",
                ps.param,
                cs.d_compliance,
                fd_comp
            );
            assert!(
                rel_err(cs.d_max_displacement, fd_u) < 1e-4,
                "param {} d_max_displacement adjoint={} vs fixed-mesh FD={}",
                ps.param,
                cs.d_max_displacement,
                fd_u
            );
            assert!(
                rel_err(cs.d_max_vm_elem, fd_vm) < 1e-4,
                "param {} d_max_vm_elem adjoint={} vs fixed-mesh FD={}",
                ps.param,
                cs.d_max_vm_elem,
                fd_vm
            );
        }
    }

    #[test]
    fn pressurized_cylinder_curved_boundary_velocity_matches_analytical_mass_and_lame_sign() {
        let src = "\
fieldsmith 1
name \"Pressurized Bore\"
custommaterial al_iso e=70000 nu=0.33 yield=450 density=2.70
material al_iso
let r_in = 6
let r_out = 12
let p_bore = 25
cylinder outer r=r_out h=30 at=(0, 0, 0)
cylinder bore r=r_in h=34 at=(0, 0, 0) subtract
region bore_wall = bore.side
region base_cap = outer.bottom
support hold base_cap fixed=xyz
load internal_p bore_wall pressure_fn=\"p_bore\"
";
        let doc = fsm::parse(src).expect("parse");
        let rep = compute_sensitivities(&doc, &[], Resolution::Dofs(15_000), zero_clock)
            .expect("cylinder adjoint sensitivities");
        assert_eq!(rep.method, "boundary_velocity_adjoint");
        assert_eq!(rep.solves_run, 3);

        let s_rin = rep.parameters.iter().find(|p| p.param == "r_in").unwrap();
        let s_rout = rep.parameters.iter().find(|p| p.param == "r_out").unwrap();
        let s_p = rep.parameters.iter().find(|p| p.param == "p_bore").unwrap();

        // Analytical mass m = π (r_out^2 - r_in^2) H ρ / 1000:
        // ∂m/∂r_in  = -2π r_in H ρ / 1000  = -2π * 6  * 30 * 2.70 / 1000 = -3.0536 g/mm
        // ∂m/∂r_out = +2π r_out H ρ / 1000 = +2π * 12 * 30 * 2.70 / 1000 = +6.1073 g/mm
        let dm_drin_exact = -2.0 * std::f64::consts::PI * 6.0 * 30.0 * 2.70 / 1000.0;
        let dm_drout_exact = 2.0 * std::f64::consts::PI * 12.0 * 30.0 * 2.70 / 1000.0;
        assert!(
            ((s_rin.d_mass_g - dm_drin_exact) / dm_drin_exact.abs()).abs() < 0.03,
            "expected ∂m/∂r_in ≈ {dm_drin_exact:.4}, got {:.4}",
            s_rin.d_mass_g
        );
        assert!(
            ((s_rout.d_mass_g - dm_drout_exact) / dm_drout_exact.abs()).abs() < 0.03,
            "expected ∂m/∂r_out ≈ {dm_drout_exact:.4}, got {:.4}",
            s_rout.d_mass_g
        );

        // Increasing bore radius thins wall AND increases pressurized area => compliance rises steeply (`> 0`).
        // Increasing outer radius thickens wall => compliance drops (`< 0`).
        assert!(
            s_rin.cases[0].d_compliance > 0.0 && s_rin.cases[0].d_max_vm_elem > 0.0,
            "increasing r_in must increase compliance and hoop von Mises: {:?}",
            s_rin.cases[0]
        );
        assert!(
            s_rout.cases[0].d_compliance < 0.0 && s_rout.cases[0].d_max_vm_elem < 0.0,
            "increasing r_out must reduce compliance and hoop von Mises: {:?}",
            s_rout.cases[0]
        );
        assert!((s_p.cases[0].rel_compliance - 2.0).abs() < 1e-6);
    }

    #[test]
    fn sketch_driven_extrude_propagates_let_through_2d_solver_to_3d_adjoint() {
        let src = "\
fieldsmith 1
name \"Sketch Driven Rib\"
custommaterial steel200 e=200000 nu=0.30 yield=250 density=7.85
material steel200
let rib_b = 10
let rib_h = 12
sketch prof on=xy {
  point p0 at=(-5, 0)
  point p1 at=(5, 0)
  point p2 at=(5, 12)
  point p3 at=(-5, 12)
  segment s0 p1=p0 p2=p1
  segment s1 p1=p1 p2=p2
  segment s2 p1=p2 p2=p3
  segment s3 p1=p3 p2=p0
  constraint c_fix kind=fixed point=p0 at=(-5, 0)
  constraint c_h0 kind=horizontal segment=s0
  constraint c_h2 kind=horizontal segment=s2
  constraint c_v1 kind=vertical segment=s1
  constraint c_v3 kind=vertical segment=s3
  constraint c_w kind=distance segment=s0 value=10 expr=\"rib_b\"
  constraint c_d kind=distance segment=s1 value=12 expr=\"rib_h\"
}
sketchfeature rib_body sketch=prof kind=boss_extrude h=60
region clamp_face = rib_body.bottom
region load_face = rib_body.top
support hold clamp_face fixed=xyz
load tip_down load_face force=(0, -300, 0)
";
        let doc = fsm::parse(src).expect("parse sketch doc");
        let rep = compute_sensitivities(
            &doc,
            &["rib_h".to_string()],
            Resolution::Dofs(12_000),
            zero_clock,
        )
        .expect("sketch sensitivity");
        assert_eq!(rep.method, "boundary_velocity_adjoint");
        assert_eq!(rep.solves_run, 3);
        let sr = &rep.parameters[0];
        // Increasing rib height (`60 mm × 10 mm` × Δrib_h × 7.85 g/cm³ / 1000 = +4.71 g/mm) increases mass
        // and strongly decreases bending compliance!
        assert!(
            (sr.d_mass_g - 4.71).abs() < 0.05,
            "expected ∂m/∂rib_h ≈ +4.71 g/mm, got {}",
            sr.d_mass_g
        );
        assert!(
            (sr.cases[0].rel_compliance - (-2.90)).abs() < 0.15,
            "expected Timoshenko bending compliance elasticity ~ -2.90 wrt sketch rib_h, got {}",
            sr.cases[0].rel_compliance
        );
    }

    #[test]
    fn thermal_and_j2_pipe_uses_fixed_mesh_boundary_velocity_without_remeshing() {
        let src = "\
fieldsmith 1
name \"Thermo-Plastic Cooling Pipe\"
material petg
let r_in = 5
cylinder outer r=8 h=40 at=(0, 0, 0)
cylinder bore r=r_in h=44 at=(0, 0, 0) subtract
region bore_wall = bore.side
region mount = outer.bottom
support fix mount fixed=xyz
load bolt_preload mount preload=500
convection ext bore_wall h=1.0 tinf=20
heatflux q_in bore_wall q=0.05
solve plasticity=j2 tref=20
";
        let doc = fsm::parse(src).expect("parse pipe");
        let rep = compute_sensitivities(
            &doc,
            &["r_in".to_string()],
            Resolution::Dofs(15_000),
            zero_clock,
        )
        .expect("pipe fixed-mesh sensitivity");
        assert_eq!(rep.method, "boundary_velocity_fixed_mesh");
        // 2 baseline cases (1 mechanical J2 + 1 thermal) + 2*2 fixed-mesh evaluations = 6 solves, 0 re-meshing
        assert_eq!(rep.solves_run, 6);
        let dm = rep.parameters[0].d_mass_g;
        // Analytical ∂m/∂r_in = -2π * 5 * 40 * 1.27 / 1000 = -1.5959 g/mm
        let dm_exact = -2.0 * std::f64::consts::PI * 5.0 * 40.0 * 1.27 / 1000.0;
        assert!(
            ((dm - dm_exact) / dm_exact.abs()).abs() < 0.05,
            "expected ∂m/∂r_in ≈ {dm_exact:.4} g/mm, got {dm:.4}"
        );
        assert!(
            rep.parameters[0].cases[1].d_max_temp.is_some(),
            "thermal case must populate d_max_temp"
        );
    }

    #[test]
    fn staggered_thermo_elastic_adjoints_match_fixed_mesh_fd_in_o1_solves() {
        let src = "\
fieldsmith 1
name \"Staggered Thermo-Elastic Adjoint Pipe\"
material ss316l
let r_in = 5
let r_out = 9
let h_val = 0.008
let q_val = 0.12
cylinder outer r=r_out h=40 at=(0, 0, 20)
cylinder bore r=r_in h=44 at=(0, 0, 20) subtract
region bore_wall = bore.side
region outer_wall = outer.side
region mount = outer.bottom
support fix mount fixed=xyz
convection bore_cool bore_wall h=0.008 tinf=25 h_fn=\"h_val * (1 + 0.01 * z)\" case=\"Thermal\"
heatflux outer_heat outer_wall q=0.12 q_fn=\"q_val * (1 + 0.005 * z)\" case=\"Thermal\"
solve tref=20
";
        let doc = fsm::parse(src).expect("parse thermo-elastic pipe");
        let rep = compute_sensitivities(&doc, &[], Resolution::Dofs(8_000), zero_clock)
            .expect("staggered thermo-elastic adjoints");
        assert_eq!(rep.method, "boundary_velocity_adjoint");
        // 1 baseline solve + 6 adjoint solves (2 mechanical + 4 scalar conduction) = 7 total across ALL 4 parameters!
        assert_eq!(rep.solves_run, 7);
        assert_eq!(rep.parameters.len(), 4);

        // Cross-check all 4 parameters against direct fixed-mesh finite differences (`evaluate_cases_on_fixed_mesh`)!
        let base = analyse(&doc, Resolution::Dofs(8_000), zero_clock).unwrap();
        let sdoc_0 = structural_doc(&doc);
        let base_h = base.provenance.element_size;
        let locked_elem_mats = lock_element_material_ids(&sdoc_0, &base.mesh, base_h).unwrap();
        let facets: Vec<Facet> = base.surface.iter().map(|&nodes| Facet { nodes }).collect();
        let mut thermal_region_map: HashMap<String, Vec<usize>> = HashMap::new();
        for reg_id in ["bore_wall", "outer_wall"] {
            thermal_region_map.insert(
                reg_id.to_string(),
                region_facets(&sdoc_0, reg_id, &base.mesh, &facets, base_h).unwrap(),
            );
        }
        let exact_lookup = |rid: &str| -> Result<Vec<usize>, SetupError> {
            Err(SetupError(format!("no mechanical region {rid}")))
        };
        let thermal_lookup =
            |rid: &str| -> Result<Vec<usize>, SetupError> { Ok(thermal_region_map[rid].clone()) };
        let e0 = expr::compile_body(&sdoc_0);
        let vert_adj = build_vertex_adjacency(&base.mesh);

        for ps in &rep.parameters {
            let addr = ParamAddr::parse(&ps.param).unwrap();
            let p0 = ps.nominal;
            let eps = ps.step;
            let vel =
                compute_mesh_velocity(&doc, &addr, p0, &e0, &base.mesh, &facets, &vert_adj, base_h)
                    .unwrap();
            let mesh_hi = deform_mesh_checked(&base.mesh, &vel, eps).unwrap();
            let mesh_lo = deform_mesh_checked(&base.mesh, &vel, -eps).unwrap();
            let mut d_hi = doc.clone();
            let mut d_lo = doc.clone();
            apply_sens_param(&mut d_hi, &addr, p0 + eps).unwrap();
            apply_sens_param(&mut d_lo, &addr, p0 - eps).unwrap();
            let (m_hi, _) = evaluate_cases_on_fixed_mesh(
                &d_hi,
                &mesh_hi,
                &facets,
                &base.fixed,
                base_h,
                &locked_elem_mats,
                &exact_lookup,
                &thermal_lookup,
            )
            .unwrap();
            let (m_lo, _) = evaluate_cases_on_fixed_mesh(
                &d_lo,
                &mesh_lo,
                &facets,
                &base.fixed,
                base_h,
                &locked_elem_mats,
                &exact_lookup,
                &thermal_lookup,
            )
            .unwrap();
            let denom = 2.0 * eps;
            let fd_comp = (m_hi[0].compliance - m_lo[0].compliance) / denom;
            let fd_u = (m_hi[0].max_displacement - m_lo[0].max_displacement) / denom;
            let fd_vm = (m_hi[0].max_vm_elem - m_lo[0].max_vm_elem) / denom;
            let fd_t = (m_hi[0].max_temp.unwrap() - m_lo[0].max_temp.unwrap()) / denom;

            let cs = &ps.cases[0];
            let adj_t = cs.d_max_temp.expect("d_max_temp present");
            let rel_err = |a: f64, b: f64| (a - b).abs() / b.abs().max(1e-9);
            assert!(
                rel_err(cs.d_compliance, fd_comp) < 5e-4,
                "param {} d_compliance adjoint={} vs FD={}",
                ps.param,
                cs.d_compliance,
                fd_comp
            );
            assert!(
                rel_err(cs.d_max_displacement, fd_u) < 5e-4,
                "param {} d_max_displacement adjoint={} vs FD={}",
                ps.param,
                cs.d_max_displacement,
                fd_u
            );
            assert!(
                rel_err(cs.d_max_vm_elem, fd_vm) < 5e-4,
                "param {} d_max_vm_elem adjoint={} vs FD={}",
                ps.param,
                cs.d_max_vm_elem,
                fd_vm
            );
            assert!(
                rel_err(adj_t, fd_t) < 5e-4,
                "param {} d_max_temp adjoint={} vs FD={}",
                ps.param,
                adj_t,
                fd_t
            );
        }
    }

    #[test]
    fn declarative_1d_system_to_3d_thermo_elastic_adjoint_sensitivity() {
        let src = "\
fieldsmith 1
name \"Declarative 1D Cooled Pipe\"
material ss316l
let mdot = 0.04
let wall_t = 3
cylinder outer_pipe r=6 + wall_t h=60 at=(0, 0, 30)
cylinder inner_bore r=6 h=64 at=(0, 0, 30) subtract
region bore_wall = inner_bore.side
region outer_wall = outer_pipe.side
region pipe_base = outer_pipe.bottom
support base_fix pipe_base fixed=xyz
convection bore_conv bore_wall h=0.005 tinf=25 case=\"Thermal\"
heatflux outer_flux outer_wall q=0.15 case=\"Thermal\"

system coolant_channel title=\"1D Coolant Channel\" sector=\"Thermal-Fluids\" nodes=25 span=(0, 60) axis=z {
  design mdot nominal=0.04 min=0.01 max=0.20 unit=\"kg/s\" symbol=\"m_dot\"
  state T_f init=\"25\" scale=100 unit=\"C\" symbol=\"T_f\"
  aux P_wet expr=\"2 * pi * 6\"
  aux h_cool expr=\"0.012 * (mdot / 0.04)^0.8\"
  residual T_f eq=\"4180 * mdot * d_ds(T_f) - P_wet * 0.15 * (9 / 6)\" bc_left=\"T_f - 25\"
  objective exit_temp expr=\"T_f\" reduce=exit kind=upper limit=80 unit=\"C\" symbol=\"T_out\"
  bind bore_conv attr=tinf expr=\"T_f\"
  bind bore_conv attr=h expr=\"h_cool\"
}
solve tref=20
assert \"Wall peak temperature bounded\" metric=\"max_temp\" target=\"<= 150\" tol=\"<= 150\" unit=\"C\"
";
        let doc = fsm::parse(src).expect("parse declarative 1D+3D doc");
        // 1. Standalone verify_document automatically solves the 1D system and verifies the 3D contract!
        let (_a, _g, vrep) =
            crate::contracts::verify_document(&doc, Some(src), Resolution::Dofs(7_000), zero_clock)
                .expect("standalone verify_document on system doc");
        assert!(vrep.all_passed, "expected contract pass: {vrep:?}");

        // 2. End-to-end 1D system -> 3D staggered thermo-elastic adjoint wrt `mdot` and `wall_t`!
        let rep = compute_sensitivities(&doc, &[], Resolution::Dofs(7_000), zero_clock)
            .expect("1D -> 3D adjoint sensitivity");
        assert_eq!(rep.method, "boundary_velocity_adjoint");
        assert_eq!(rep.solves_run, 7);
        let s_mdot = rep
            .parameters
            .iter()
            .find(|p| p.param == "mdot")
            .expect("mdot sensitivity");
        let dt_dmdot = s_mdot.cases[0].d_max_temp.expect("d_max_temp wrt mdot");
        // Increasing coolant mass flow `mdot` raises `h_cool` AND lowers coolant temperature rise `T_f(z)`,
        // strictly lowering 3D wall peak temperature (`∂T_max / ∂mdot < 0`) and thermal stress!
        assert!(
            dt_dmdot < -10.0,
            "expected ∂T_max,3D / ∂mdot << 0, got {dt_dmdot}"
        );
        assert!(
            s_mdot.cases[0].d_max_vm_elem < 0.0,
            "expected ∂σ_vM,3D / ∂mdot < 0, got {}",
            s_mdot.cases[0].d_max_vm_elem
        );
    }

    #[test]
    fn modal_rayleigh_quotient_matches_material_and_euler_bernoulli_scaling() {
        // Clamped 3D cantilever without static loads (`solve modes=2` only):
        // Mode 1 bends in Z (depth d = 8 mm), Mode 2 bends in Y (width b = 12 mm).
        let src = "\
fieldsmith 1
name \"Modal Rayleigh Quotient Cantilever\"
custommaterial steel200 e=200000 nu=0.30 yield=250 density=7.85
material steel200
let L = 60
let b = 12
let d = 8
box beam size=(L, b, d) at=(L / 2, 0, 0)
region root = beam.nx
support clamp root fixed=xyz
solve modes=2
";
        let doc = fsm::parse(src).expect("parse modal cantilever");
        let params = vec![
            "L".to_string(),
            "b".to_string(),
            "d".to_string(),
            "steel200.e".to_string(),
            "steel200.density".to_string(),
        ];
        let rep = compute_sensitivities(&doc, &params, Resolution::Dofs(9_000), zero_clock)
            .expect("modal Rayleigh-quotient sensitivity");
        assert_eq!(rep.method, "boundary_velocity_adjoint");
        // Pure modal document: 1 baseline modal solve, 0 extra linear/eigen solves across all 5 params!
        assert_eq!(rep.solves_run, 1);
        assert_eq!(rep.baseline_modes_hz.len(), 2);

        let by_name = |name: &str| {
            rep.parameters
                .iter()
                .find(|p| p.param == name)
                .unwrap_or_else(|| panic!("missing param {name}"))
        };
        let p_e = by_name("steel200.e");
        let p_rho = by_name("steel200.density");
        let p_l = by_name("L");
        let p_b = by_name("b");
        let p_d = by_name("d");

        // Exact scaling laws: f ∝ E^(+1/2) and f ∝ ρ^(-1/2)
        assert!(
            (p_e.rel_modes[0] - 0.5).abs() < 1e-4 && (p_e.rel_modes[1] - 0.5).abs() < 1e-4,
            "expected (E/f)(∂f/∂E) == +0.5, got {:?}",
            p_e.rel_modes
        );
        assert!(
            (p_rho.rel_modes[0] + 0.5).abs() < 1e-4 && (p_rho.rel_modes[1] + 0.5).abs() < 1e-4,
            "expected (ρ/f)(∂f/∂ρ) == -0.5, got {:?}",
            p_rho.rel_modes
        );

        // Euler-Bernoulli / Timoshenko geometric elasticities:
        // Mode 1 (Z-bending, thinner depth d=8): e_d ≈ +0.97, e_b ≈ 0, e_L ≈ -1.96
        assert!(
            (p_d.rel_modes[0] - 0.97).abs() < 0.06,
            "expected (d/f1)(∂f1/∂d) ≈ +0.97, got {}",
            p_d.rel_modes[0]
        );
        assert!(
            p_b.rel_modes[0].abs() < 0.05,
            "expected |(b/f1)(∂f1/∂b)| < 0.05, got {}",
            p_b.rel_modes[0]
        );
        assert!(
            (p_l.rel_modes[0] + 1.96).abs() < 0.08,
            "expected (L/f1)(∂f1/∂L) ≈ -1.96, got {}",
            p_l.rel_modes[0]
        );
        // Mode 2 (Y-bending, wider width b=12): e_b ≈ +0.95, e_d ≈ 0
        assert!(
            (p_b.rel_modes[1] - 0.95).abs() < 0.08,
            "expected (b/f2)(∂f2/∂b) ≈ +0.95, got {}",
            p_b.rel_modes[1]
        );
        assert!(
            p_d.rel_modes[1].abs() < 0.05,
            "expected |(d/f2)(∂f2/∂d)| < 0.05, got {}",
            p_d.rel_modes[1]
        );

        // Cross-check ∂f_1/∂d against independent re-meshing finite difference!
        let remesh = compute_sensitivities_remesh(
            &doc,
            &["d".to_string()],
            Resolution::Dofs(9_000),
            zero_clock,
        )
        .expect("remesh modal FD");
        let df1_adj = p_d.d_modes_hz[0];
        let df1_fd = remesh.parameters[0].d_modes_hz[0];
        let rel_diff = (df1_adj - df1_fd).abs() / df1_fd.abs();
        assert!(
            rel_diff < 0.05,
            "expected Rayleigh-quotient ∂f1/∂d ({df1_adj}) within 5% of remesh FD ({df1_fd}), rel_diff={rel_diff}"
        );
    }

    #[test]
    fn graded_tpms_adjoint_sensitivities_scale_o1_and_match_fixed_mesh_fd() {
        // 3D stress-graded cubic-anisotropic gyroid-sheet cantilever with outer depth `let d`
        // AND `fill.*` lattice parameters (`rho_min`, `rho_max`, `skin`, `cell`).
        let src = "\
fieldsmith 1
name \"Graded Gyroid Cantilever Sensitivity\"
material petg
let d = 18
box beam size=(56, 18, d) at=(28, 0, 0)
region clamp = beam.nx
region tip = beam.px
support root clamp fixed=xyz
load push tip force=(0, 0, -240)
fill gyroid sheet cell=6 skin=1.2 rho=(0.22, 0.55) curve=1.0 wall=0.4
";
        let doc = fsm::parse(src).expect("parse graded cantilever");
        let rep = compute_sensitivities(&doc, &[], Resolution::Dofs(7_500), zero_clock)
            .expect("graded TPMS discrete adjoint sensitivities");

        assert_eq!(rep.method, "boundary_velocity_adjoint");
        // 1 solid + 1 graded + at most 4 adjoint RHS (u_max, vm, yield_util, buckling_util) = 6 solves across 5 parameters!
        assert!(
            rep.solves_run <= 6,
            "expected O(1) <= 6 solves across 5 parameters, got {}",
            rep.solves_run
        );
        let names: Vec<&str> = rep.parameters.iter().map(|p| p.param.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "d",
                "fill.rho_min",
                "fill.rho_max",
                "fill.skin",
                "fill.cell"
            ]
        );
        assert!(rep.baseline_solid_mass_g.unwrap() > rep.baseline_mass_g);

        // 1. Physical sign checks on graded lattice parameters vs outer geometry:
        let by_name = |n: &str| rep.parameters.iter().find(|p| p.param == n).unwrap();
        let p_rmin = by_name("fill.rho_min");
        let p_rmax = by_name("fill.rho_max");
        let p_skin = by_name("fill.skin");
        let p_d = by_name("d");

        // `fill.*` never alters un-filled solid mass (`∂m_solid / ∂fill.* == 0`), while `d` increases both!
        assert_eq!(p_rmin.d_solid_mass_g.unwrap(), 0.0);
        assert_eq!(p_rmax.d_solid_mass_g.unwrap(), 0.0);
        assert_eq!(p_skin.d_solid_mass_g.unwrap(), 0.0);
        assert!(p_d.d_solid_mass_g.unwrap() > p_d.d_mass_g && p_d.d_mass_g > 0.0);

        // Increasing `rho_min`, `rho_max`, `skin`, or beam depth `d` increases graded mass and stiffens the beam (`∂C/∂p < 0`)
        // AND reduces lattice yield & buckling utilization (`∂U_y/∂p < 0`, `∂U_b/∂p < 0`)!
        for p in [p_rmin, p_rmax, p_skin, p_d] {
            assert!(
                p.d_mass_g > 0.0,
                "expected ∂m_graded/∂{} > 0, got {}",
                p.param,
                p.d_mass_g
            );
            let cs = &p.cases[0];
            assert!(
                cs.d_compliance < 0.0,
                "expected ∂C/∂{} < 0, got {}",
                p.param,
                cs.d_compliance
            );
            assert!(
                cs.d_yield_utilization.unwrap() < 0.0,
                "expected ∂U_y/∂{} < 0, got {:?}",
                p.param,
                cs.d_yield_utilization
            );
            assert!(
                cs.d_buckling_utilization.unwrap() < 0.0,
                "expected ∂U_b/∂{} < 0, got {:?}",
                p.param,
                cs.d_buckling_utilization
            );
        }

        // 2. Exact discrete-adjoint verification on cubic anisotropic TPMS elements against direct
        //    fixed-mesh graded solves (`eval_graded_perturbed_props` + `run_case`)!
        let solid_base = analyse(&doc, Resolution::Dofs(7_500), zero_clock).unwrap();
        let graded_0 = crate::fill::graded_of(&doc, &solid_base, zero_clock, false).unwrap();
        let sdoc_0 = structural_doc(&doc);
        let base_h = solid_base.provenance.element_size;
        let locked_elem_mats =
            lock_element_material_ids(&sdoc_0, &solid_base.mesh, base_h).unwrap();
        let elem_props_0 = eval_element_materials(&sdoc_0, &locked_elem_mats).unwrap();
        let smooth_0 = crate::fill::prepare_graded_smooth_baseline(
            &sdoc_0,
            &solid_base,
            &graded_0,
            &elem_props_0.rho_elem,
        )
        .unwrap();
        let facets: Vec<Facet> = solid_base
            .surface
            .iter()
            .map(|&nodes| Facet { nodes })
            .collect();
        let tip_facets =
            region_facets_exact(&sdoc_0, "tip", &solid_base.mesh, &facets, base_h).unwrap();
        let exact_lookup = |rid: &str| -> Result<Vec<usize>, SetupError> {
            assert_eq!(rid, "tip");
            Ok(tip_facets.clone())
        };
        let e0 = expr::compile_body(&sdoc_0);
        let vert_adj = build_vertex_adjacency(&solid_base.mesh);
        let cname = case_names(&sdoc_0)[0].clone();

        for pname in ["fill.rho_min", "fill.rho_max", "fill.skin", "d"] {
            let ps = by_name(pname);
            let addr = ParamAddr::parse(pname).unwrap();
            let p0 = ps.nominal;
            let eps = ps.step;
            let vel = if is_fill_param_target(&doc, &addr) {
                vec![[0.0; 3]; solid_base.mesh.nodes.len()]
            } else {
                compute_mesh_velocity(
                    &doc,
                    &addr,
                    p0,
                    &e0,
                    &solid_base.mesh,
                    &facets,
                    &vert_adj,
                    base_h,
                )
                .unwrap()
            };

            let eval_side = |delta: f64| {
                let mut d_p = doc.clone();
                apply_sens_param(&mut d_p, &addr, p0 + delta).unwrap();
                let sdoc_p = structural_doc(&d_p);
                let mesh_p = deform_mesh_checked(&solid_base.mesh, &vel, delta).unwrap();
                let ep = eval_element_materials(&sdoc_p, &locked_elem_mats).unwrap();
                let gp = crate::fill::eval_graded_perturbed_props(
                    &sdoc_p,
                    &mesh_p,
                    base_h,
                    &graded_0,
                    &smooth_0,
                    &ep.rho_elem,
                )
                .unwrap();
                let (force, _, prescribed_map) = case_force_report_with_regions(
                    &sdoc_p,
                    &mesh_p,
                    &facets,
                    &cname,
                    base_h,
                    Some(&exact_lookup),
                )
                .unwrap();
                let mut node_key: HashMap<[u64; 3], u8> = HashMap::new();
                for (i, pt) in mesh_p.nodes.iter().enumerate() {
                    node_key.insert(
                        [pt[0].to_bits(), pt[1].to_bits(), pt[2].to_bits()],
                        solid_base.fixed[i],
                    );
                }
                let prescribed = prescribed_fn(&node_key, &prescribed_map);
                let cr = crate::run_case(
                    &mesh_p,
                    &gp.material,
                    &solid_base.fixed,
                    force,
                    cname.clone(),
                    false,
                    &prescribed,
                )
                .unwrap();
                let mut uy = 0.0f64;
                let mut ub = 0.0f64;
                for e in 0..mesh_p.tets.len() {
                    if graded_0.class[e] != crate::fill::CLASS_VOID {
                        let sy = crate::fill::strength_extended(
                            gp.spec.family,
                            gp.spec.mode,
                            gp.base_mat.yield_strength,
                            gp.phi_elem[e],
                        );
                        uy = uy.max(cr.vm_elem[e] / sy);
                    }
                    if graded_0.class[e] == crate::fill::CLASS_LATTICE && cr.sigma_min_elem[e] < 0.0
                    {
                        let sb = crate::fill::buckling_extended(
                            gp.spec.family,
                            gp.spec.mode,
                            gp.base_mat.e,
                            gp.phi_elem[e],
                        );
                        ub = ub.max((-cr.sigma_min_elem[e]) / sb);
                    }
                }
                (cr, uy, ub)
            };

            let (cr_hi, uy_hi, ub_hi) = eval_side(eps);
            let (cr_lo, uy_lo, ub_lo) = eval_side(-eps);
            let denom = 2.0 * eps;
            let fd_comp = (cr_hi.compliance - cr_lo.compliance) / denom;
            let fd_u = (cr_hi.max_displacement - cr_lo.max_displacement) / denom;
            let fd_uy = (uy_hi - uy_lo) / denom;
            let fd_ub = (ub_hi - ub_lo) / denom;

            let cs = &ps.cases[0];
            let rel_err = |a: f64, b: f64| (a - b).abs() / b.abs().max(1e-8);
            assert!(
                rel_err(cs.d_compliance, fd_comp) < 2e-3,
                "param {pname} d_compliance adj={} vs fd={fd_comp}",
                cs.d_compliance
            );
            assert!(
                rel_err(cs.d_max_displacement, fd_u) < 2e-3,
                "param {pname} d_max_displacement adj={} vs fd={fd_u}",
                cs.d_max_displacement
            );
            assert!(
                rel_err(cs.d_yield_utilization.unwrap(), fd_uy) < 5e-3,
                "param {pname} d_yield_utilization adj={} vs fd={fd_uy}",
                cs.d_yield_utilization.unwrap()
            );
            assert!(
                rel_err(cs.d_buckling_utilization.unwrap(), fd_ub) < 5e-3,
                "param {pname} d_buckling_utilization adj={} vs fd={fd_ub}",
                cs.d_buckling_utilization.unwrap()
            );
        }
    }
}
