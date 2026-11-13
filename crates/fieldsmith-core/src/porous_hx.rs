//! 3D P2 Darcy–Forchheimer Porous-Media Flow & Two-Stream Graded-TPMS Conjugate Heat Exchange
//! (`VISION.md §5.2 Pillar IV.3`).
//!
//! Homogenises any graded TPMS core (`Gyroid`, `Primitive`, `Diamond`, including spatially
//! graded relative density `ρ(x)`, spatially graded unit-cell size `cell_min`, and multi-zone
//! family `morph`) on the actual 3D quadratic (`P2`) tetrahedral mesh of the user's part:
//!
//! 1. **Unit-cell calibrated porous-hydraulic laws** (`REV = a³`):
//!    - Specific minimal-surface interfacial area density `a_v(x) = c_A0 / a(x)` (`1/m` for
//!      minimal-surface `Sheet` lattices where zero mean curvature `H = 0` preserves parallel
//!      offset surface area to `O((w/a)²)`; modulated by `√(4 ρ (1 − ρ))` on `Network` struts).
//!    - Hydraulic diameter `D_h(x) = 2 φ_f(x) / a_v(x)` (`Sheet`, two wetted faces enclosing
//!      void fraction `φ_f = 1 − ρ`) or `4 φ_f(x) / a_v(x)` (`Network`).
//!    - Kozeny–Carman permeability `K(x) = φ_f(x)³ / (k_KC · s_wetted(x)²)` (`m²`) and Ergun–Ward
//!      Forchheimer inertial drag `β(x) = C_F / (φ_f(x)^1.5 · √K(x))` (`1/m`).
//!
//! 2. **3D P2 nonlinear Darcy–Forchheimer solver**:
//!    `-∇p = (μ / K(x)) u_D + β(x) ρ_f |u_D| u_D`, `∇·u_D = 0`, inverted pointwise via exact
//!    chord mobility `κ_eff(x, |∇p|) = 2 / (a_D + √(a_D² + 4 b_F |∇p|))` so `u_D = -κ_eff ∇p`
//!    reduces each fixed-point step to the SPD scalar P2 conduction operator
//!    [`fieldsmith_solve::solve_conduction`], accelerated by safeguarded log-mobility secant
//!    (Aitken/Barzilai–Borwein) iteration. On a uniform TPMS prism, `p(x)` is linear and lies
//!    inside the P2 finite-element space, reproducing the exact 1D Darcy–Forchheimer solution
//!    to solver tolerance regardless of tetrahedral mesh skewness.
//!
//! 3. **Conjugate wall conductance & two-stream `ε–NTU` effectiveness**:
//!    Integrates local hot/cold interstitial Nusselt numbers `Nu(Re_h, Pr)` and conductive TPMS
//!    sheet wall resistance `w(x) / k_solid` element-by-element over the 3D velocity field to
//!    obtain overall `UA = ∫_core U(x) a_v(x) dV`, `NTU = UA / C_min`, effectiveness `ε`,
//!    heat duty `Q_w`, volumetric power density (`kW/L`), and outlet temperatures for
//!    `counterflow`, `parallel`, `crossflow`, and `isothermal_wall` arrangements.

use fieldsmith_doc::{
    materials::Material, Document, Family, Fill, FillMode, HxArrangement, HxSpec,
};
use fieldsmith_field::expr::{self, Expr};
use fieldsmith_field::lattice::{calibration, FillExpr};
use fieldsmith_field::V3;
use fieldsmith_solve::assemble::{element_map, RefData};
use fieldsmith_solve::{element_flux, solve_conduction, HeatFluxBc, P2Mesh, SolverOptions};
use serde::{Deserialize, Serialize};

use crate::{region_facets, Analysis, Facet, SetupError};

/// Unit-cell geometric, hydraulic, and convective constants for one TPMS family.
#[derive(Clone, Copy, Debug)]
pub struct TpmsCellLaw {
    /// Dimensionless minimal-surface area per unit cube `A_mid / a²`.
    pub c_area: f64,
    /// Calibrated Kozeny constant `k_KC` in `K = φ_f³ / (k_KC · s_wetted²)`.
    pub kozeny_k: f64,
    /// Calibrated Forchheimer inertial form-drag constant `C_F` in `β = C_F / (φ_f^1.5 √K)`.
    pub forchheimer_c: f64,
    /// Asymptotic laminar Nusselt number `Nu_0` as `Re_h → 0`.
    pub nu_0: f64,
    /// Calibrated convective Reynolds prefactor `c_Nu` in `Nu = Nu_0 + c_Nu Re_h^0.65 Pr^(1/3)`.
    pub c_nu: f64,
}

/// Unit-cell hydraulic and convective correlation constants for `family`.
pub fn tpms_cell_law(family: Family) -> TpmsCellLaw {
    match family {
        Family::Primitive => TpmsCellLaw {
            c_area: 2.3451,
            kozeny_k: 4.20,
            forchheimer_c: 0.085,
            nu_0: 3.66,
            c_nu: 0.42,
        },
        Family::Gyroid => TpmsCellLaw {
            c_area: 3.0915,
            kozeny_k: 5.00,
            forchheimer_c: 0.145,
            nu_0: 4.36,
            c_nu: 0.62,
        },
        Family::Diamond => TpmsCellLaw {
            c_area: 3.8382,
            kozeny_k: 5.80,
            forchheimer_c: 0.195,
            nu_0: 4.80,
            c_nu: 0.76,
        },
    }
}

impl TpmsCellLaw {
    fn lerp(self, other: Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        let s = 1.0 - t;
        Self {
            c_area: s * self.c_area + t * other.c_area,
            kozeny_k: s * self.kozeny_k + t * other.kozeny_k,
            forchheimer_c: s * self.forchheimer_c + t * other.forchheimer_c,
            nu_0: s * self.nu_0 + t * other.nu_0,
            c_nu: s * self.c_nu + t * other.c_nu,
        }
    }
}

/// Local homogenised porous-hydraulic properties of a TPMS unit cell (`REV = a³`).
#[derive(Clone, Copy, Debug)]
pub struct LocalPorousProps {
    /// Fluid void fraction `φ_f = 1 − ρ_rev ∈ [0.06, 0.96]`.
    pub porosity: f64,
    /// Intrinsic Darcy permeability `K` (`m²`).
    pub permeability_m2: f64,
    /// Forchheimer inertial drag coefficient `β` (`1/m`).
    pub beta_per_m: f64,
    /// One-sided partition surface area density `a_v,part` (`1/m`).
    pub partition_area_per_m: f64,
    /// Total solid-fluid wetted surface area density `s_wetted` (`1/m`).
    pub wetted_area_per_m: f64,
    /// Interstitial hydraulic diameter `D_h` (`m`).
    pub hydraulic_diam_m: f64,
    /// Local wall/strut conductive thickness `w` (`m`).
    pub wall_m: f64,
    /// Blended unit-cell Nusselt constants.
    pub law: TpmsCellLaw,
}

/// Evaluate local homogenised TPMS porous properties at relative solid density `rho_rev` and
/// unit-cell size `a_mm` (`mm`).
pub fn local_porous_props(
    law: TpmsCellLaw,
    mode: FillMode,
    rho_rev: f64,
    a_mm: f64,
    w_mm: f64,
) -> LocalPorousProps {
    let rho_clamped = rho_rev.clamp(0.04, 0.94);
    let porosity = (1.0 - rho_clamped).clamp(0.06, 0.96);
    let a_m = (a_mm.max(1e-3)) * 1e-3;
    let w_m = (w_mm.abs().max(0.05 * a_mm)) * 1e-3;
    let (partition_area_per_m, wetted_area_per_m, hydraulic_diam_m) = match mode {
        FillMode::Sheet => {
            let a_part = law.c_area / a_m;
            let s_wet = 2.0 * a_part;
            let d_h = 4.0 * porosity / s_wet;
            (a_part, s_wet, d_h)
        }
        FillMode::Network => {
            let shape_mod = (4.0 * rho_clamped * (1.0 - rho_clamped)).max(0.04).sqrt();
            let a_part = (law.c_area / a_m) * shape_mod;
            let d_h = 4.0 * porosity / a_part.max(1e-6);
            (a_part, a_part, d_h)
        }
    };
    let permeability_m2 =
        (porosity * porosity * porosity) / (law.kozeny_k * wetted_area_per_m * wetted_area_per_m);
    let beta_per_m = law.forchheimer_c / (porosity.powf(1.5) * permeability_m2.max(1e-24).sqrt());
    LocalPorousProps {
        porosity,
        permeability_m2,
        beta_per_m,
        partition_area_per_m,
        wetted_area_per_m,
        hydraulic_diam_m,
        wall_m: w_m,
        law,
    }
}

/// Exact Effectiveness–NTU (`ε–NTU`) relation for `arrangement`, `ntu >= 0`, and capacity-rate
/// ratio `cr = C_min / C_max ∈ [0, 1]`.
pub fn effectiveness_ntu(arrangement: HxArrangement, ntu: f64, cr: f64) -> f64 {
    let ntu = ntu.max(0.0);
    if ntu == 0.0 {
        return 0.0;
    }
    let cr = cr.clamp(0.0, 1.0);
    if matches!(arrangement, HxArrangement::IsothermalWall) || cr <= 1e-12 {
        return 1.0 - (-ntu).exp();
    }
    match arrangement {
        HxArrangement::IsothermalWall => 1.0 - (-ntu).exp(),
        HxArrangement::Counterflow => {
            if (1.0 - cr).abs() < 1e-9 {
                ntu / (1.0 + ntu)
            } else {
                let e = (-ntu * (1.0 - cr)).exp();
                (1.0 - e) / (1.0 - cr * e)
            }
        }
        HxArrangement::Parallel => {
            let e = (-ntu * (1.0 + cr)).exp();
            (1.0 - e) / (1.0 + cr)
        }
        HxArrangement::Crossflow => {
            // Kays & London both-fluids-unmixed crossflow relation:
            let exponent = (1.0 / cr) * ntu.powf(0.22) * ((-cr * ntu.powf(0.78)).exp() - 1.0);
            (1.0 - exponent.exp()).clamp(0.0, 1.0)
        }
    }
}

/// Solved 3D P2 Darcy–Forchheimer hydraulic and two-stream conjugate `ε–NTU` summary for a
/// graded TPMS lattice core.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TpmsHxReport {
    pub inlet: String,
    pub outlet: String,
    pub fluid: String,
    pub cold_fluid: String,
    pub arrangement: String,
    pub inlet_area_mm2: f64,
    pub outlet_area_mm2: f64,
    pub core_volume_mm3: f64,
    pub mean_porosity: f64,
    pub effective_permeability_mm2: f64,
    pub forchheimer_beta_per_m: f64,
    pub mean_area_density_per_m: f64,
    pub wetted_area_cm2: f64,
    pub mean_hydraulic_diam_mm: f64,
    pub inlet_pressure_kpa: f64,
    pub outlet_pressure_kpa: f64,
    pub pressure_drop_kpa: f64,
    pub cold_pressure_drop_kpa: f64,
    pub darcy_share_pct: f64,
    pub hot_pumping_power_w: f64,
    pub pumping_power_w: f64,
    pub mean_superficial_vel_m_s: f64,
    pub max_superficial_vel_m_s: f64,
    pub mean_reynolds: f64,
    pub picard_iters: usize,
    pub mean_h_hot_w_m2_k: f64,
    pub mean_h_cold_w_m2_k: f64,
    pub wall_k_w_m_k: f64,
    pub ua_w_k: f64,
    pub ntu: f64,
    pub capacity_ratio_cr: f64,
    pub effectiveness: f64,
    pub heat_duty_w: f64,
    pub power_density_kw_per_l: f64,
    pub hot_inlet_temp_c: f64,
    pub hot_outlet_temp_c: f64,
    pub cold_inlet_temp_c: f64,
    pub cold_outlet_temp_c: f64,
}

/// Cached per-element physical basis gradients and volume on `P2Mesh`.
struct ElemGradCache {
    vol_mm3: f64,
    /// `(weight, [grad_phi_0 .. grad_phi_9])` at each degree-2 quadrature point; `∑ weight = 1/6`.
    qpts: Vec<(f64, [[f64; 3]; 10])>,
}

fn build_elem_grad_cache(mesh: &P2Mesh) -> Vec<ElemGradCache> {
    let rd = RefData::new();
    let mut out = Vec::with_capacity(mesh.tets.len());
    for e in 0..mesh.tets.len() {
        let map = element_map(mesh, e);
        let det = map.det_jacobian.abs();
        let mut qpts = Vec::with_capacity(rd.weights.len());
        for (q, &w) in rd.weights.iter().enumerate() {
            let mut g = [[0.0f64; 3]; 10];
            for (a, grad_ref) in rd.grads[q].iter().enumerate() {
                g[a] = map.transform_gradient(grad_ref);
            }
            qpts.push((w, g));
        }
        out.push(ElemGradCache {
            vol_mm3: det / 6.0,
            qpts,
        });
    }
    out
}

/// Exact RMS pressure gradient magnitude `√((1/|T|) ∫_T |∇_mm p|² dV)` (`Pa/mm`) on element `e`.
#[inline]
fn elem_rms_grad_p_mm(mesh: &P2Mesh, e: usize, cache: &ElemGradCache, p_pa: &[f64]) -> f64 {
    let t = &mesh.tets[e];
    let p_loc: [f64; 10] = t.map(|idx| p_pa[idx as usize]);
    let mut integral_ref = 0.0f64;
    for &(w, ref grads) in &cache.qpts {
        let mut gx = 0.0f64;
        let mut gy = 0.0f64;
        let mut gz = 0.0f64;
        for a in 0..10 {
            gx += p_loc[a] * grads[a][0];
            gy += p_loc[a] * grads[a][1];
            gz += p_loc[a] * grads[a][2];
        }
        integral_ref += w * (gx * gx + gy * gy + gz * gz);
    }
    (6.0 * integral_ref.max(0.0)).sqrt()
}

fn tet_corner_centroid(mesh: &P2Mesh, e: usize) -> V3 {
    let t = &mesh.tets[e];
    let mut c = [0.0f64; 3];
    for &n in &t[..4] {
        let p = mesh.nodes[n as usize];
        c[0] += p[0];
        c[1] += p[1];
        c[2] += p[2];
    }
    [c[0] * 0.25, c[1] * 0.25, c[2] * 0.25]
}

/// Squared Euclidean distance from point `p` to 3D triangle `(a, b, c)`.
fn point_triangle_dist_sq(p: V3, a: V3, b: V3, c: V3) -> f64 {
    let sub = |u: V3, v: V3| [u[0] - v[0], u[1] - v[1], u[2] - v[2]];
    let dot = |u: V3, v: V3| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let norm2 = |u: V3| dot(u, u);

    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return norm2(ap);
    }

    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return norm2(bp);
    }

    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        let proj = [a[0] + v * ab[0], a[1] + v * ab[1], a[2] + v * ab[2]];
        return norm2(sub(p, proj));
    }

    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return norm2(cp);
    }

    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        let proj = [a[0] + w * ac[0], a[1] + w * ac[1], a[2] + w * ac[2]];
        return norm2(sub(p, proj));
    }

    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let bc = sub(c, b);
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        let proj = [b[0] + w * bc[0], b[1] + w * bc[1], b[2] + w * bc[2]];
        return norm2(sub(p, proj));
    }

    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    let proj = [
        a[0] + ab[0] * v + ac[0] * w,
        a[1] + ab[1] * v + ac[1] * w,
        a[2] + ab[2] * v + ac[2] * w,
    ];
    norm2(sub(p, proj))
}

fn min_dist_to_facets(mesh: &P2Mesh, facets: &[Facet], idxs: &[usize], p: V3) -> f64 {
    let mut best_sq = f64::INFINITY;
    for &fi in idxs {
        let [a, b, c] = facets[fi].corners(mesh);
        let d2 = point_triangle_dist_sq(p, a, b, c);
        if d2 < best_sq {
            best_sq = d2;
        }
    }
    best_sq.sqrt()
}

#[inline]
fn chord_mobility_si(a_darcy: f64, b_forch: f64, g_si: f64) -> f64 {
    let disc = (a_darcy * a_darcy + 4.0 * b_forch * g_si.max(0.0)).sqrt();
    2.0 / (a_darcy + disc).max(1e-30)
}

#[inline]
fn nusselt_tpms(law: TpmsCellLaw, re_h: f64, pr: f64) -> f64 {
    law.nu_0 + law.c_nu * re_h.max(0.0).powf(0.65) * pr.max(1e-6).cbrt()
}

/// Solve the 3D P2 Darcy–Forchheimer pressure field and two-stream conjugate `ε–NTU` performance
/// on `base.mesh` for `hx` and `fill`. Returns `(report, nodal_pressure_kpa)`.
pub fn solve_tpms_porous_hx(
    doc: &Document,
    base: &Analysis,
    spec: &Fill,
    fill: &FillExpr,
    mat: &Material,
    hx: &HxSpec,
) -> Result<(TpmsHxReport, Vec<f64>), SetupError> {
    if !hx.mdot.is_finite() || hx.mdot <= 0.0 {
        return Err(SetupError("hx mdot must be positive".into()));
    }
    let cold_mdot = hx.cold_mdot.unwrap_or(hx.mdot);
    if !cold_mdot.is_finite() || cold_mdot <= 0.0 {
        return Err(SetupError("hx cold_mdot must be positive".into()));
    }

    let mesh = &base.mesh;
    let h_elem = base.provenance.element_size;
    let facets: Vec<Facet> = base.surface.iter().map(|&nodes| Facet { nodes }).collect();

    let inlet_idx = region_facets(doc, &hx.inlet, mesh, &facets, h_elem)?;
    if inlet_idx.is_empty() {
        return Err(SetupError(format!(
            "hx inlet region '{}' matched zero surface facets",
            hx.inlet
        )));
    }
    let outlet_idx = region_facets(doc, &hx.outlet, mesh, &facets, h_elem)?;
    if outlet_idx.is_empty() {
        return Err(SetupError(format!(
            "hx outlet region '{}' matched zero surface facets",
            hx.outlet
        )));
    }

    // Exact P2 isoparametric facet weights `l_f[a] = ∫_f φ_a dΓ` (mm²) on inlet and outlet.
    let mut inlet_facets = Vec::with_capacity(inlet_idx.len());
    let mut inlet_weights = Vec::with_capacity(inlet_idx.len());
    let mut inlet_area_mm2 = 0.0f64;
    for &fi in &inlet_idx {
        let nodes = facets[fi].nodes;
        let l = element_flux(mesh, &nodes);
        inlet_area_mm2 += l.iter().sum::<f64>();
        inlet_facets.push(nodes);
        inlet_weights.push(l);
    }

    let mut outlet_weights = Vec::with_capacity(outlet_idx.len());
    let mut outlet_area_mm2 = 0.0f64;
    let p_out_pa = hx.pout_kpa * 1000.0;
    let mut dirichlet = vec![None; mesh.nodes.len()];
    for &fi in &outlet_idx {
        let nodes = facets[fi].nodes;
        let l = element_flux(mesh, &nodes);
        outlet_area_mm2 += l.iter().sum::<f64>();
        outlet_weights.push((nodes, l));
        for &ni in &nodes {
            dirichlet[ni as usize] = Some(p_out_pa);
        }
    }

    // Ensure at least one inlet node is free (not completely covered by outlet Dirichlet).
    let has_free_inlet = inlet_facets
        .iter()
        .any(|f| f.iter().any(|&ni| dirichlet[ni as usize].is_none()));
    if !has_free_inlet || inlet_area_mm2 <= 1e-12 || outlet_area_mm2 <= 1e-12 {
        return Err(SetupError(format!(
            "hx inlet '{}' and outlet '{}' overlap completely on the mesh boundary",
            hx.inlet, hx.outlet
        )));
    }

    let (rho_h, mu_h, cp_h, kf_h) = hx.fluid.properties();
    let cold_fluid = hx.cold_fluid.unwrap_or(hx.fluid);
    let (rho_c, mu_c, cp_c, kf_c) = cold_fluid.properties();
    let pr_h = (mu_h * cp_h) / kf_h;
    let pr_c = (mu_c * cp_c) / kf_c;

    let inlet_area_m2 = inlet_area_mm2 * 1e-6;
    let u_d_in_m_s = hx.mdot / (rho_h * inlet_area_m2);

    // Precompute element REV porous properties and classify open lattice core vs solid skin/bosses.
    let grad_cache = build_elem_grad_cache(mesh);
    let ne = mesh.tets.len();
    let cal_lo = calibration(spec.family, spec.mode);
    let law_lo = tpms_cell_law(spec.family);
    let morph_fam = spec.morph.filter(|&m| m != spec.family);
    let cal_hi = morph_fam.map(|f| calibration(f, spec.mode));
    let law_hi = morph_fam.map(tpms_cell_law);

    let skin_max = spec.skin_max.unwrap_or(spec.skin);
    let has_skin = skin_max > 0.0 || spec.skin > 0.0;
    let has_keep = !matches!(fill.keep, Expr::Empty);
    let port_collar_mm = skin_max.max(spec.skin) + 0.75 * h_elem;

    let mut elem_props = Vec::with_capacity(ne);
    let mut is_core = vec![true; ne];
    let mut is_solid_obstacle = vec![false; ne];
    let mut min_core_darcy_kappa = f64::INFINITY;

    for e in 0..ne {
        let c = tet_corner_centroid(mesh, e);
        let pv = fill.param.sample(c);
        let a_mm = fill.cell_at_param(pv).max(1e-3);
        let w_mm = fill.wall_at_param(pv);
        let (rho_rev, law) = if let (Some(ch), Some(lh)) = (cal_hi, law_hi) {
            let t = fill.grade_at_param(pv);
            let r0 = cal_lo.rho_for(w_mm, a_mm);
            let r1 = ch.rho_for(w_mm, a_mm);
            ((1.0 - t) * r0 + t * r1, law_lo.lerp(lh, t))
        } else {
            (cal_lo.rho_for(w_mm, a_mm), law_lo)
        };
        let props = local_porous_props(law, spec.mode, rho_rev, a_mm, w_mm);
        let darcy_kappa = props.permeability_m2 / mu_h;

        let in_keep = has_keep && expr::eval(&fill.keep, c).v <= 0.0;
        if in_keep {
            is_core[e] = false;
            is_solid_obstacle[e] = true;
        } else if has_skin {
            let d_outer = fill.outer_body(c).v;
            let sk = fill.skin_at_param(pv);
            if d_outer + sk > 0.0 {
                // Element centroid lies in the outer solid skin shell: open an aperture collar
                // immediately behind `inlet` and `outlet` ports while keeping lateral skins solid.
                let d_in = min_dist_to_facets(mesh, &facets, &inlet_idx, c);
                let d_out = min_dist_to_facets(mesh, &facets, &outlet_idx, c);
                if d_in.min(d_out) <= port_collar_mm {
                    // Open port header: hydraulically permeable like adjacent core, but not
                    // counted as TPMS heat-transfer core if inside solid port collar.
                    is_core[e] = false;
                    is_solid_obstacle[e] = false;
                } else {
                    is_core[e] = false;
                    is_solid_obstacle[e] = true;
                }
            }
        }

        if !is_solid_obstacle[e] && darcy_kappa < min_core_darcy_kappa {
            min_core_darcy_kappa = darcy_kappa;
        }
        elem_props.push(props);
    }

    // Fallback if skin/keep covered the entire part: treat permeable elements (or all elements)
    // as porous so the solve remains well-posed instead of dividing by zero.
    if !min_core_darcy_kappa.is_finite() {
        min_core_darcy_kappa = elem_props
            .iter()
            .map(|p| p.permeability_m2 / mu_h)
            .fold(f64::INFINITY, f64::min)
            .max(1e-18);
        is_solid_obstacle.fill(false);
    }
    if !is_core.iter().any(|&b| b) {
        for e in 0..ne {
            is_core[e] = !is_solid_obstacle[e];
        }
    }

    let solid_kappa_si = (1e-5 * min_core_darcy_kappa).max(1e-20);

    // Safeguarded secant / Barzilai–Borwein fixed-point iteration in log-mobility `y_e = ln(κ_SI,e)`:
    // `-∇_mm · ((1000 κ_SI) ∇_mm p_pa) = 0` with Neumann flux `u_d_in_m_s` on `Γ_in` and `p_out_pa` on `Γ_out`.
    let mut y: Vec<f64> = (0..ne)
        .map(|e| {
            let k_si = if is_solid_obstacle[e] {
                solid_kappa_si
            } else {
                elem_props[e].permeability_m2 / mu_h
            };
            k_si.ln()
        })
        .collect();

    let flux_bc = [HeatFluxBc {
        q: u_d_in_m_s,
        facets: inlet_facets.clone(),
    }];
    let solver_opts = SolverOptions {
        rtol: 1e-10,
        max_iter: 4_000,
    };

    let max_picard = 28usize;
    let picard_tol = 1e-7;
    let mut p_pa = vec![p_out_pa; mesh.nodes.len()];
    let mut y_prev: Option<Vec<f64>> = None;
    let mut r_prev: Option<Vec<f64>> = None;
    let mut picard_iters = 0usize;

    for it in 1..=max_picard {
        picard_iters = it;
        let k_mesh: Vec<f64> = y.iter().map(|&ye| 1000.0 * ye.exp()).collect();
        let (p_sol, _rep) = solve_conduction(mesh, &dirichlet, &k_mesh, &[], &flux_bc, solver_opts)
            .map_err(|e| SetupError(format!("porous Darcy–Forchheimer solve failed: {e}")))?;
        p_pa = p_sol;

        // Evaluate raw log-mobility map `F(y)` and fixed-point residual `r = F(y) - y`.
        let mut r = vec![0.0f64; ne];
        let mut max_abs_r = 0.0f64;
        for e in 0..ne {
            if is_solid_obstacle[e] {
                r[e] = 0.0;
                continue;
            }
            let g_si = 1000.0 * elem_rms_grad_p_mm(mesh, e, &grad_cache[e], &p_pa);
            let a_d = mu_h / elem_props[e].permeability_m2;
            let b_f = elem_props[e].beta_per_m * rho_h;
            let y_raw = chord_mobility_si(a_d, b_f, g_si).ln();
            let re = y_raw - y[e];
            r[e] = re;
            if re.abs() > max_abs_r {
                max_abs_r = re.abs();
            }
        }

        if max_abs_r < picard_tol {
            for e in 0..ne {
                y[e] += r[e];
            }
            break;
        }

        // Compute safeguarded Barzilai–Borwein / Aitken step length `ω ∈ [0.6, 2.0]`:
        // Because `F'(y) ∈ [0, 0.5]` pointwise for Darcy–Forchheimer, `(I - F')⁻¹ ∈ [1, 2]`.
        let omega = if let (Some(ref yp), Some(ref rp)) = (&y_prev, &r_prev) {
            let mut sd = 0.0f64;
            let mut dd = 0.0f64;
            for e in 0..ne {
                if is_solid_obstacle[e] {
                    continue;
                }
                let w_vol = grad_cache[e].vol_mm3;
                let s_e = y[e] - yp[e];
                let d_e = r[e] - rp[e];
                sd += w_vol * s_e * d_e;
                dd += w_vol * d_e * d_e;
            }
            if dd > 1e-30 && sd < 0.0 {
                (-sd / dd).clamp(0.75, 2.0)
            } else {
                1.0
            }
        } else {
            1.0
        };

        let y_old = y.clone();
        for e in 0..ne {
            if !is_solid_obstacle[e] {
                y[e] += omega * r[e];
            }
        }
        y_prev = Some(y_old);
        r_prev = Some(r);
    }

    // Final linear solve with converged chord mobility if `picard_iters > 1` so `p_pa` is
    // strictly consistent with `y`.
    if picard_iters > 1 {
        let k_mesh: Vec<f64> = y.iter().map(|&ye| 1000.0 * ye.exp()).collect();
        let (p_sol, _) = solve_conduction(mesh, &dirichlet, &k_mesh, &[], &flux_bc, solver_opts)
            .map_err(|e| SetupError(format!("porous Darcy–Forchheimer solve failed: {e}")))?;
        p_pa = p_sol;
    }

    // Area-weighted inlet and outlet static pressures (Pa -> kPa).
    let mut p_in_sum = 0.0f64;
    for (nodes, l) in inlet_facets.iter().zip(&inlet_weights) {
        for a in 0..6 {
            p_in_sum += l[a] * p_pa[nodes[a] as usize];
        }
    }
    let p_in_pa = p_in_sum / inlet_area_mm2;
    let mut p_out_sum = 0.0f64;
    for (nodes, l) in &outlet_weights {
        for a in 0..6 {
            p_out_sum += l[a] * p_pa[nodes[a] as usize];
        }
    }
    let p_out_mean_pa = p_out_sum / outlet_area_mm2;
    let dp_hot_pa = (p_in_pa - p_out_mean_pa).max(0.0);
    let pressure_drop_kpa = dp_hot_pa * 1e-3;

    // Integrate 3D hydraulic dissipation, Darcy viscous fraction, cold-stream pressure drop,
    // and local conjugate wall conductance `UA` over the TPMS core elements.
    let wall_k_w_m_k = mat.effective_k_w_mm_k() * 1000.0;
    let is_two_stream = !matches!(hx.arrangement, HxArrangement::IsothermalWall);
    let cold_vel_ratio = if is_two_stream {
        (cold_mdot * rho_h) / (hx.mdot * rho_c)
    } else {
        0.0
    };

    let mut core_volume_mm3 = 0.0f64;
    let mut sum_porosity = 0.0f64;
    let mut sum_k_m2 = 0.0f64;
    let mut sum_beta = 0.0f64;
    let mut sum_a_v = 0.0f64;
    let mut sum_dh_mm = 0.0f64;
    let mut wetted_area_m2 = 0.0f64;

    let mut diss_darcy_w = 0.0f64;
    let mut diss_total_w = 0.0f64;
    let mut diss_cold_w = 0.0f64;

    let mut sum_u_hot = 0.0f64;
    let mut max_u_hot = 0.0f64;
    let mut sum_re_hot = 0.0f64;
    let mut sum_h_hot = 0.0f64;
    let mut sum_h_cold = 0.0f64;
    let mut ua_w_k = 0.0f64;

    for e in 0..ne {
        if !is_core[e] {
            continue;
        }
        let pr = elem_props[e];
        let v_mm3 = grad_cache[e].vol_mm3;
        let v_m3 = v_mm3 * 1e-9;

        let g_si = 1000.0 * elem_rms_grad_p_mm(mesh, e, &grad_cache[e], &p_pa);
        let a_d = mu_h / pr.permeability_m2;
        let b_f = pr.beta_per_m * rho_h;
        let kappa_si = chord_mobility_si(a_d, b_f, g_si);
        let u_hot = kappa_si * g_si;

        let w_darcy = a_d * u_hot * u_hot * v_m3;
        let w_forch = b_f * u_hot * u_hot * u_hot * v_m3;
        diss_darcy_w += w_darcy;
        diss_total_w += w_darcy + w_forch;

        if is_two_stream {
            let u_cold = u_hot * cold_vel_ratio;
            let a_dc = mu_c / pr.permeability_m2;
            let b_fc = pr.beta_per_m * rho_c;
            diss_cold_w += (a_dc * u_cold * u_cold + b_fc * u_cold * u_cold * u_cold) * v_m3;
        }

        let re_h = (rho_h * u_hot * pr.hydraulic_diam_m) / (pr.porosity * mu_h);
        let nu_h = nusselt_tpms(pr.law, re_h, pr_h);
        let h_hot = (nu_h * kf_h) / pr.hydraulic_diam_m;

        let (h_cold, d_area_m2, r_wall_m2_k_w) = if is_two_stream {
            let u_cold = u_hot * cold_vel_ratio;
            let re_c = (rho_c * u_cold * pr.hydraulic_diam_m) / (pr.porosity * mu_c);
            let nu_c = nusselt_tpms(pr.law, re_c, pr_c);
            let hc = (nu_c * kf_c) / pr.hydraulic_diam_m;
            (
                hc,
                pr.partition_area_per_m * v_m3,
                pr.wall_m / wall_k_w_m_k.max(1e-6),
            )
        } else {
            // Single-stream coldplate / regenerator (`IsothermalWall`): both faces of a Sheet
            // lattice wet the single working stream, conducting across half-wall thickness `w/2`.
            (
                0.0,
                pr.wetted_area_per_m * v_m3,
                0.5 * pr.wall_m / wall_k_w_m_k.max(1e-6),
            )
        };

        let inv_u = if is_two_stream {
            (1.0 / h_hot.max(1e-9)) + r_wall_m2_k_w + (1.0 / h_cold.max(1e-9))
        } else {
            (1.0 / h_hot.max(1e-9)) + r_wall_m2_k_w
        };
        ua_w_k += d_area_m2 / inv_u;
        wetted_area_m2 += d_area_m2;

        let a_reported = if is_two_stream {
            pr.partition_area_per_m
        } else {
            pr.wetted_area_per_m
        };

        core_volume_mm3 += v_mm3;
        sum_porosity += v_mm3 * pr.porosity;
        sum_k_m2 += v_mm3 * pr.permeability_m2;
        sum_beta += v_mm3 * pr.beta_per_m;
        sum_a_v += v_mm3 * a_reported;
        sum_dh_mm += v_mm3 * (pr.hydraulic_diam_m * 1000.0);
        sum_u_hot += v_mm3 * u_hot;
        if u_hot > max_u_hot {
            max_u_hot = u_hot;
        }
        sum_re_hot += v_mm3 * re_h;
        sum_h_hot += v_mm3 * h_hot;
        sum_h_cold += v_mm3 * h_cold;
    }

    let inv_vol = 1.0 / core_volume_mm3.max(1e-12);
    let darcy_share_pct = if diss_total_w > 1e-30 {
        100.0 * (diss_darcy_w / diss_total_w).clamp(0.0, 1.0)
    } else {
        100.0
    };

    let q_vol_hot_m3_s = hx.mdot / rho_h;
    let hot_pumping_power_w = q_vol_hot_m3_s * dp_hot_pa;

    // Scale cold pressure drop from exact ratio of cold-to-hot viscous+inertial dissipation
    // (exact on both 1D prisms and 3D curved streamlines because `P_pump = Q_vol · Δp = ∫ Φ dV`).
    let (cold_pressure_drop_kpa, cold_pumping_power_w) = if is_two_stream && diss_total_w > 1e-30 {
        let scale_diss = diss_cold_w / diss_total_w;
        let p_cold_w = hot_pumping_power_w * scale_diss;
        let q_vol_cold_m3_s = cold_mdot / rho_c;
        let dp_cold_kpa = (p_cold_w / q_vol_cold_m3_s.max(1e-18)) * 1e-3;
        (dp_cold_kpa, p_cold_w)
    } else {
        (0.0, 0.0)
    };
    let pumping_power_w = hot_pumping_power_w + cold_pumping_power_w;

    // Effectiveness–NTU & outlet temperatures.
    let c_hot = hx.mdot * cp_h;
    let (c_min, capacity_ratio_cr, c_cold_opt) = if is_two_stream {
        let c_cold = cold_mdot * cp_c;
        let c_lo = c_hot.min(c_cold);
        let c_hi = c_hot.max(c_cold).max(1e-18);
        (c_lo, c_lo / c_hi, Some(c_cold))
    } else {
        (c_hot, 0.0, None)
    };
    let ntu = ua_w_k / c_min.max(1e-18);
    let effectiveness = effectiveness_ntu(hx.arrangement, ntu, capacity_ratio_cr);

    let dt_in = hx.tin - hx.cold_tin;
    let q_signed = effectiveness * c_min * dt_in;
    let heat_duty_w = q_signed.abs();
    let hot_outlet_temp_c = hx.tin - q_signed / c_hot.max(1e-18);
    let cold_outlet_temp_c = match c_cold_opt {
        Some(c_cold) => hx.cold_tin + q_signed / c_cold.max(1e-18),
        None => hx.cold_tin,
    };

    let core_liters = (core_volume_mm3 * 1e-6).max(1e-12);
    let power_density_kw_per_l = (heat_duty_w * 1e-3) / core_liters;

    let pressure_kpa: Vec<f64> = p_pa.into_iter().map(|p| p * 1e-3).collect();

    let report = TpmsHxReport {
        inlet: hx.inlet.clone(),
        outlet: hx.outlet.clone(),
        fluid: hx.fluid.name().to_string(),
        cold_fluid: cold_fluid.name().to_string(),
        arrangement: hx.arrangement.name().to_string(),
        inlet_area_mm2,
        outlet_area_mm2,
        core_volume_mm3,
        mean_porosity: sum_porosity * inv_vol,
        effective_permeability_mm2: (sum_k_m2 * inv_vol) * 1e6,
        forchheimer_beta_per_m: sum_beta * inv_vol,
        mean_area_density_per_m: sum_a_v * inv_vol,
        wetted_area_cm2: wetted_area_m2 * 1e4,
        mean_hydraulic_diam_mm: sum_dh_mm * inv_vol,
        inlet_pressure_kpa: p_in_pa * 1e-3,
        outlet_pressure_kpa: p_out_mean_pa * 1e-3,
        pressure_drop_kpa,
        cold_pressure_drop_kpa,
        darcy_share_pct,
        hot_pumping_power_w,
        pumping_power_w,
        mean_superficial_vel_m_s: sum_u_hot * inv_vol,
        max_superficial_vel_m_s: max_u_hot,
        mean_reynolds: sum_re_hot * inv_vol,
        picard_iters,
        mean_h_hot_w_m2_k: sum_h_hot * inv_vol,
        mean_h_cold_w_m2_k: sum_h_cold * inv_vol,
        wall_k_w_m_k,
        ua_w_k,
        ntu,
        capacity_ratio_cr,
        effectiveness,
        heat_duty_w,
        power_density_kw_per_l,
        hot_inlet_temp_c: hx.tin,
        hot_outlet_temp_c,
        cold_inlet_temp_c: hx.cold_tin,
        cold_outlet_temp_c,
    };

    Ok((report, pressure_kpa))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fill, now_ms, Resolution};
    use fieldsmith_doc::fsm;

    #[test]
    fn p2_darcy_forchheimer_matches_1d_analytical_formula_in_viscous_and_inertial_regimes() {
        // Uniform 60 × 20 × 20 mm Gyroid Sheet core (`ρ = 0.25`, `cell = 5.0 mm`, `skin = 0`).
        let src_template = |mdot: f64| {
            format!(
                r#"fieldsmith 1
material alsi10mg
box core size=(60, 20, 20)
region left = core.nx
region right = core.px
support anchor left fixed=xyz
load pull right pressure=-0.1
fill gyroid sheet cell=5 skin=0 rho=(0.25, 0.25) wall=0.1
hx inlet=left outlet=right mdot={mdot} tin=80 fluid=water arrangement=counterflow cold_mdot={mdot} cold_tin=20 pout=100
"#
            )
        };

        let law = tpms_cell_law(Family::Gyroid);
        let cal = calibration(Family::Gyroid, FillMode::Sheet);
        let w_mm = cal.param_for(0.25, 5.0);
        let rho_exact = cal.rho_for(w_mm, 5.0);
        let props = local_porous_props(law, FillMode::Sheet, rho_exact, 5.0, w_mm);
        let (rho_f, mu_f, cp_f, kf_f) = fieldsmith_doc::HxFluid::Water.properties();
        let area_m2 = (20.0 * 20.0) * 1e-6;
        let len_m = 60.0 * 1e-3;

        for &mdot in &[0.003_f64, 0.080_f64] {
            let doc = fsm::parse(&src_template(mdot)).expect("parse");
            let (_base, g) = fill::graded(&doc, Resolution::Dofs(5_000), now_ms).expect("graded");
            let hx = g.hx.as_ref().expect("hx report");

            let u_d = mdot / (rho_f * area_m2);
            let dp_darcy_pa = len_m * (mu_f / props.permeability_m2) * u_d;
            let dp_forch_pa = len_m * props.beta_per_m * rho_f * u_d * u_d;
            let dp_exact_kpa = (dp_darcy_pa + dp_forch_pa) * 1e-3;
            let darcy_pct_exact = 100.0 * dp_darcy_pa / (dp_darcy_pa + dp_forch_pa);

            let rel_err_dp = (hx.pressure_drop_kpa - dp_exact_kpa).abs() / dp_exact_kpa;
            assert!(
                rel_err_dp < 1e-4,
                "mdot={mdot}: 3D P2 Δp {:.6} kPa vs analytical {:.6} kPa (rel err {:.2e})",
                hx.pressure_drop_kpa,
                dp_exact_kpa,
                rel_err_dp
            );
            assert!(
                (hx.darcy_share_pct - darcy_pct_exact).abs() < 0.02,
                "mdot={mdot}: darcy_share_pct {:.4}% vs {:.4}%",
                hx.darcy_share_pct,
                darcy_pct_exact
            );
            assert!(
                hx.picard_iters <= 12,
                "expected fast BB/secant convergence, took {} iters",
                hx.picard_iters
            );

            // Check exact conjugate ε-NTU against closed-form formula (`C_r = 1` counterflow).
            let pr = (mu_f * cp_f) / kf_f;
            let re_h = (rho_f * u_d * props.hydraulic_diam_m) / (props.porosity * mu_f);
            let h_conv = nusselt_tpms(law, re_h, pr) * kf_f / props.hydraulic_diam_m;
            let k_s = fieldsmith_doc::materials::find(&doc, "alsi10mg")
                .unwrap()
                .effective_k_w_mm_k()
                * 1000.0;
            let inv_u = 2.0 / h_conv + props.wall_m / k_s;
            let vol_m3 = (60.0 * 20.0 * 20.0) * 1e-9;
            let ua_exact = (props.partition_area_per_m * vol_m3) / inv_u;
            let ntu_exact = ua_exact / (mdot * cp_f);
            let eps_exact = ntu_exact / (1.0 + ntu_exact);
            let q_exact = eps_exact * (mdot * cp_f) * 60.0;

            assert!((hx.ua_w_k - ua_exact).abs() / ua_exact < 1e-4);
            assert!((hx.effectiveness - eps_exact).abs() < 1e-5);
            assert!((hx.heat_duty_w - q_exact).abs() / q_exact < 1e-4);
            assert!((hx.hot_outlet_temp_c + hx.cold_outlet_temp_c - 100.0).abs() < 1e-4);
        }
    }

    #[test]
    fn effectiveness_arrangements_and_family_ordering_behave_physically() {
        let eps_cf = effectiveness_ntu(HxArrangement::Counterflow, 2.2, 0.8);
        let eps_xf = effectiveness_ntu(HxArrangement::Crossflow, 2.2, 0.8);
        let eps_pf = effectiveness_ntu(HxArrangement::Parallel, 2.2, 0.8);
        let eps_iso = effectiveness_ntu(HxArrangement::IsothermalWall, 2.2, 0.0);
        assert!(
            eps_iso > eps_cf && eps_cf > eps_xf && eps_xf > eps_pf,
            "expected iso ({eps_iso:.4}) > counter ({eps_cf:.4}) > cross ({eps_xf:.4}) > parallel ({eps_pf:.4})"
        );

        let solve_fam = |fam: &str, with_boss: bool| {
            let protect = if with_boss {
                "cylinder boss r=4.5 h=20\n"
            } else {
                ""
            };
            let prot_attr = if with_boss {
                " protect=[boss] margin=0.5"
            } else {
                ""
            };
            let src = format!(
                r#"fieldsmith 1
material alsi10mg
box core size=(40, 20, 20)
{protect}region left = core.nx
region right = core.px
support anchor left fixed=xyz
load pull right pressure=-0.1
fill {fam} sheet cell=5 skin=0 rho=(0.25, 0.25) wall=0.2{prot_attr}
hx inlet=left outlet=right mdot=0.05 tin=90 fluid=water arrangement=counterflow cold_mdot=0.035 cold_tin=20
"#
            );
            let doc = fsm::parse(&src).expect("parse");
            let (_b, g) = fill::graded(&doc, Resolution::Dofs(5_000), now_ms).expect("graded");
            g.hx.expect("hx")
        };

        let p_rep = solve_fam("primitive", false);
        let g_rep = solve_fam("gyroid", false);
        let d_rep = solve_fam("diamond", false);
        assert!(
            d_rep.pressure_drop_kpa > g_rep.pressure_drop_kpa
                && g_rep.pressure_drop_kpa > p_rep.pressure_drop_kpa,
            "expected Δp ordering Diamond ({:.2}) > Gyroid ({:.2}) > Primitive ({:.2})",
            d_rep.pressure_drop_kpa,
            g_rep.pressure_drop_kpa,
            p_rep.pressure_drop_kpa
        );
        assert!(
            d_rep.ua_w_k > g_rep.ua_w_k && g_rep.ua_w_k > p_rep.ua_w_k,
            "expected UA ordering Diamond ({:.1}) > Gyroid ({:.1}) > Primitive ({:.1})",
            d_rep.ua_w_k,
            g_rep.ua_w_k,
            p_rep.ua_w_k
        );

        // Energy conservation on unbalanced counterflow (`0.05` vs `0.035 kg/s`):
        let (_, _, cp_w, _) = fieldsmith_doc::HxFluid::Water.properties();
        let q_hot = 0.05 * cp_w * (90.0 - g_rep.hot_outlet_temp_c);
        let q_cold = 0.035 * cp_w * (g_rep.cold_outlet_temp_c - 20.0);
        assert!((q_hot - g_rep.heat_duty_w).abs() < 1e-6);
        assert!((q_cold - g_rep.heat_duty_w).abs() < 1e-6);

        // Internal protected boss detours 3D flow, raising pressure drop and peak velocity:
        let g_boss = solve_fam("gyroid", true);
        assert!(
            g_boss.pressure_drop_kpa > g_rep.pressure_drop_kpa * 1.10,
            "expected boss detour Δp ({:.3}) > open core Δp ({:.3})",
            g_boss.pressure_drop_kpa,
            g_rep.pressure_drop_kpa
        );
        assert!(g_boss.max_superficial_vel_m_s > g_rep.max_superficial_vel_m_s * 1.15);
    }
}
