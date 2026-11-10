//! General 3D parameter sensitivity operator (`∂Φ / ∂p`) for arbitrary `.fsm` documents.
//!
//! Evaluates derivatives of 3D P2 finite-element observables (compliance `C`, mass `m`,
//! peak displacement `u_max`, peak element von Mises stress `σ_vM`) with respect to
//! document `let` variables or dotted `ParamAddr` addresses, locking the element size `h`
//! from the baseline mesh so `+δ` and `-δ` share identical lattice cell counts on outer
//! face perturbations.

use crate::{analyse, structural_doc, Analysis, Clock, Resolution, SetupError};
use fieldsmith_doc::param::{self, ParamAddr};
use fieldsmith_doc::{materials, Document, LoadKind, NodeKind};
use fieldsmith_field::expr;
use serde::{Deserialize, Serialize};

/// Sensitivity derivatives of a single solved load case with respect to one parameter `p`.
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
}

/// Sensitivity entry for one named `let` variable or `ParamAddr`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParameterSensitivity {
    pub param: String,
    pub nominal: f64,
    pub step: f64,
    pub scheme: String,
    /// `∂m / ∂p` (g per unit of `p`).
    pub d_mass_g: f64,
    /// Dimensionless normalized mass elasticity `(p / m) · (∂m / ∂p)`.
    pub rel_mass: f64,
    pub cases: Vec<CaseSensitivity>,
}

/// Full sensitivity report across a document's `let` parameters (or requested parameter list).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SensitivityReport {
    /// Differentiation method provenance (`"finite_difference_remesh"` — locked baseline `h`,
    /// re-stuffed tetrahedral mesh at `p ± δ`; distinct from fixed-topology shape adjoints).
    pub method: String,
    /// Total 3D FEA solves executed (`1` baseline + perturbed evaluations).
    pub solves_run: usize,
    pub h: f64,
    pub nodes: usize,
    pub elements: usize,
    pub baseline_mass_g: f64,
    pub baseline_compliance: Vec<(String, f64)>,
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

/// Consistent finite-element mass (grams) integrated directly over the stuffed tetrahedral mesh
/// whose boundary vertices sit on `f(x) = 0`, avoiding `volume::integrate`'s bbox-anchored
/// voxel step bias under small `±δ` shape perturbations.
fn mesh_mass_grams(doc: &Document, a: &Analysis) -> f64 {
    let sdoc = structural_doc(doc);
    let doc = sdoc.as_ref();
    let base_rho = materials::find(doc, &doc.material)
        .map(|m| m.density)
        .unwrap_or(1.0);
    let mut labelled = Vec::new();
    for n in &doc.root {
        if n.suppressed || n.op == fieldsmith_doc::Op::Subtract {
            continue;
        }
        if let Some(mid) = &n.material {
            if mid != &doc.material {
                if let Some(m) = materials::find(doc, mid) {
                    labelled.push((expr::compile_node(doc, n), m.density));
                }
            }
        }
    }
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
            for (le, lrho) in &labelled {
                if expr::eval(le, c).v <= 1e-6 * h {
                    r = *lrho;
                }
            }
            r
        };
        mass_mm3_g_cm3 += vol * rho;
    }
    mass_mm3_g_cm3 / 1000.0
}

fn read_param_value(doc: &Document, addr: &ParamAddr) -> Result<f64, String> {
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
            (LoadKind::Pressure { mpa }, "mpa", None) => Ok(*mpa),
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
    Err(format!("param {addr}: could not read current numeric value"))
}

/// Compute 3D P2 sensitivities for `params` (if non-empty) or all non-derived `let` variables in
/// `doc.lets`.
pub fn compute_sensitivities(
    doc: &Document,
    params: &[String],
    res: Resolution,
    clock: Clock,
) -> Result<SensitivityReport, SetupError> {
    let t0 = clock();
    let base = analyse(doc, res, clock)?;
    let base_mass = mesh_mass_grams(doc, &base);
    let base_h = base.provenance.element_size;
    let locked_res = Resolution::Size(base_h);

    let targets: Vec<String> = if !params.is_empty() {
        params.to_vec()
    } else {
        let mut names = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for l in &doc.lets {
            let is_derived = seen.iter().any(|prev| l.expr.contains(prev.as_str()));
            seen.push(l.name.clone());
            if is_derived {
                continue;
            }
            names.push(l.name.clone());
        }
        names
    };

    let mut solves_run = 1usize;
    let mut out_params = Vec::with_capacity(targets.len());
    for pname in &targets {
        let addr = ParamAddr::parse(pname).map_err(SetupError)?;
        let p0 = read_param_value(doc, &addr).map_err(SetupError)?;
        let delta = (5e-3 * p0.abs()).max(1e-3);

        let eval_at = |val: f64| -> Result<(Analysis, f64), SetupError> {
            let mut d_pert = doc.clone();
            param::apply(&mut d_pert, &addr, val).map_err(SetupError)?;
            let a = analyse(&d_pert, locked_res, clock)?;
            let m = mesh_mass_grams(&d_pert, &a);
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
                a_lo_ref = &base;
                m_lo = base_mass;
                denom = delta;
                scheme = "forward";
            }
            (Err(_), Ok((am, mm))) => {
                solves_run += 1;
                am_holder = am;
                a_hi_ref = &base;
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
            cases.push(CaseSensitivity {
                case: bc.name.clone(),
                d_compliance,
                rel_compliance: rel_elasticity(p0, bc.compliance, d_compliance),
                d_max_displacement,
                d_max_vm_elem,
            });
        }

        out_params.push(ParameterSensitivity {
            param: pname.clone(),
            nominal: p0,
            step: delta,
            scheme: scheme.to_string(),
            d_mass_g,
            rel_mass,
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
        baseline_compliance: base
            .cases
            .iter()
            .map(|c| (c.name.clone(), c.compliance))
            .collect(),
        parameters: out_params,
        elapsed_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldsmith_doc::fsm;

    fn zero_clock() -> f64 {
        0.0
    }

    #[test]
    fn cantilever_thickness_and_width_sensitivities_match_beam_scaling_laws() {
        let src = "\
fieldsmith 1
name \"Slender Sensitivity Cantilever\"
custommaterial steel200 e=200000 nu=0.30 yield=250 density=7.85
material steel200
let width = 10
let depth = 8
box beam size=(80, width, depth) at=(40, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -400)
";
        let doc = fsm::parse(src).expect("parse");
        let rep = compute_sensitivities(&doc, &[], Resolution::Dofs(15_000), zero_clock)
            .expect("sensitivities");
        assert_eq!(rep.method, "finite_difference_remesh");
        assert_eq!(rep.solves_run, 5);
        assert_eq!(rep.parameters.len(), 2);

        let sw = rep.parameters.iter().find(|p| p.param == "width").unwrap();
        let sh = rep.parameters.iter().find(|p| p.param == "depth").unwrap();

        assert_eq!(sw.scheme, "central");
        assert_eq!(sh.scheme, "central");
        assert!(
            (sw.rel_mass - 1.0).abs() < 0.02,
            "expected width mass elasticity ~ +1.0, got {}",
            sw.rel_mass
        );
        assert!(
            (sh.rel_mass - 1.0).abs() < 0.02,
            "expected depth mass elasticity ~ +1.0, got {}",
            sh.rel_mass
        );

        let rel_cw = sw.cases[0].rel_compliance;
        let rel_ch = sh.cases[0].rel_compliance;
        assert!(
            (rel_cw - (-1.0)).abs() < 0.08,
            "expected width compliance elasticity ~ -1.0, got {rel_cw}"
        );
        assert!(
            (rel_ch - (-2.93)).abs() < 0.15,
            "expected depth compliance elasticity ~ -2.93 (Timoshenko -3 + shear), got {rel_ch}"
        );
        assert!(rel_ch < 2.5 * rel_cw);
    }
}
