//! Bridge from a `.fsm` [`DeclarativeSystem`] (`system <id> { ... }`) to the universal
//! [`CoupledEngineeringSystem`] / [`MonolithicSystemEngine`] solver, Sparse Adjoint & Tangent
//! sensitivity analyser, FOSM `p10/p50/p90` uncertainty propagator, and 3D `.fsm` BC projector.

use fieldsmith_doc::command::{apply_doc_command, DocCommand};
use fieldsmith_doc::fsm;
use fieldsmith_doc::spatial::SpatialFn;
use fieldsmith_doc::system::{
    sync_system_bindings_to_doc, sync_system_bindings_to_doc_exact, CompiledSystem,
    DeclDesignCategory, DeclObjKind, DeclarativeSystem,
};
use fieldsmith_doc::Document;
use fieldsmith_solve::coupled_system::{
    sparse_jacobian_from_triplets, CausalAttributionPoint, CausalAttributionTrack,
    ConservationAudit, CoupledEngineeringSystem, DesignParameter, DesignParameterCategory,
    EngineConfig, EpistemicParameter, FieldDescriptor, MonolithicSystemEngine, NamedScenario,
    ObjectiveDescriptor, ObjectiveKind, SparseJacobian,
};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// If `doc.system_block` is present and declares `bind` projections, solve its monolithic 0D/1D
/// equilibrium `R(W; p, θ) = 0` (propagating any matching top-level `doc.lets` values into
/// `design_p`) and return an owned [`Document`] with full IEEE-754 `lerp(...)` spatial boundary
/// conditions and bound `let` parameters projected in-memory. Returns `Cow::Borrowed(doc)` with
/// zero allocation when `doc.system_block` is `None`.
pub fn resolve_system_doc(doc: &Document) -> Result<Cow<'_, Document>, crate::SetupError> {
    let Some(ref sb) = doc.system_block else {
        return Ok(Cow::Borrowed(doc));
    };
    if sb.bindings.is_empty() {
        return Ok(Cow::Borrowed(doc));
    }
    let sys = DeclarativeCoupledSystem::from_document(doc).map_err(crate::SetupError)?;
    let design_p: Vec<f64> = sb
        .designs
        .iter()
        .map(|dp| {
            doc.lets
                .iter()
                .find(|l| l.name == dp.name)
                .map(|l| l.value)
                .unwrap_or(dp.nominal)
        })
        .collect();
    let theta = sys.nominal_epistemic_vector();
    let cfg = sys.engine_config();
    let (state, _, _) = MonolithicSystemEngine::solve_equilibrium(&sys, &cfg, &design_p, &theta)
        .map_err(crate::SetupError)?;
    let mut next_doc = doc.clone();
    sync_system_bindings_to_doc_exact(&mut next_doc, &state, &design_p, &theta)
        .map_err(crate::SetupError)?;
    Ok(Cow::Owned(next_doc))
}

/// Solved 0D/1D station profiles and 3D BC projection preview returned alongside
/// [`fieldsmith_solve::coupled_system::SystemAnalysisResult`] for declarative `.fsm` systems.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeclarativeSystemVisual {
    pub axis: String,
    pub nodes: usize,
    pub stations: Vec<f64>,
    pub fields: Vec<DeclarativeFieldProfile>,
    pub bindings: Vec<DeclarativeBindingSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeclarativeFieldProfile {
    pub name: String,
    pub symbol: String,
    pub unit: String,
    pub values: Vec<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeclarativeBindingSummary {
    pub target: String,
    pub attr: String,
    pub mean: f64,
    pub min: f64,
    pub max: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lerp_expr: Option<String>,
}

/// [`CoupledEngineeringSystem`] backed by a user-authored `.fsm` [`DeclarativeSystem`].
#[derive(Clone, Debug)]
pub struct DeclarativeCoupledSystem {
    base_doc: Document,
    spec: DeclarativeSystem,
    compiled: CompiledSystem,
    base_design_p: Vec<f64>,
    has_design_linked_lets: bool,
    title_display: String,
    sector_display: String,
}

impl DeclarativeCoupledSystem {
    /// Construct and compile a [`DeclarativeCoupledSystem`] from `doc` (`doc.system_block` must be `Some`).
    pub fn from_document(doc: &Document) -> Result<Self, String> {
        let spec = doc
            .system_block
            .clone()
            .ok_or_else(|| "document has no declarative 'system' block".to_string())?;
        let val_errs = spec.validate(doc);
        if !val_errs.is_empty() {
            return Err(val_errs.join("; "));
        }
        let let_vals: Vec<(String, f64)> =
            doc.lets.iter().map(|l| (l.name.clone(), l.value)).collect();
        let compiled = CompiledSystem::compile(&spec, &let_vals).map_err(|errs| errs.join("; "))?;
        let base_design_p: Vec<f64> = spec.designs.iter().map(|d| d.nominal).collect();
        let has_design_linked_lets = spec
            .designs
            .iter()
            .any(|d| doc.lets.iter().any(|l| l.name == d.name));
        let title_display = if spec.title.trim().is_empty() {
            spec.id.clone()
        } else {
            spec.title.clone()
        };
        let sector_display = if spec.sector.trim().is_empty() {
            "Declarative .fsm System".to_string()
        } else {
            spec.sector.clone()
        };
        Ok(Self {
            base_doc: doc.clone(),
            spec,
            compiled,
            base_design_p,
            has_design_linked_lets,
            title_display,
            sector_display,
        })
    }

    pub fn spec(&self) -> &DeclarativeSystem {
        &self.spec
    }

    pub fn named_scenarios(&self) -> Vec<NamedScenario> {
        self.spec
            .scenarios
            .iter()
            .map(|sc| NamedScenario {
                name: sc.name.clone(),
                description: sc.description.clone(),
                design_p: sc.design_p.clone(),
            })
            .collect()
    }

    /// Recommended [`EngineConfig`] tailored to whether any residual enables BDF1 pseudo-transient damping.
    pub fn engine_config(&self) -> EngineConfig {
        let any_transient = self.spec.residuals.iter().any(|r| r.transient != 0.0);
        if any_transient {
            EngineConfig {
                dt_s: 2.0,
                num_bdf_steps: 2,
                max_newton_iters: 20,
                newton_tol: 1e-9,
                state_fd_eps: 1e-6,
            }
        } else {
            EngineConfig {
                dt_s: 0.0,
                num_bdf_steps: 0,
                max_newton_iters: 25,
                newton_tol: 1e-9,
                state_fd_eps: 1e-6,
            }
        }
    }

    /// Return a compiled system reflecting `design_p` (re-evaluating chained `doc.lets` if any
    /// `design` lever shares its name with a `let` in the parent document).
    fn compiled_for_design(&self, design_p: &[f64]) -> Cow<'_, CompiledSystem> {
        if !self.has_design_linked_lets || design_p == self.base_design_p.as_slice() {
            return Cow::Borrowed(&self.compiled);
        }
        let mut tmp_doc = self.base_doc.clone();
        for (k, dp) in self.spec.designs.iter().enumerate() {
            if let Some(&val) = design_p.get(k) {
                if tmp_doc.lets.iter().any(|l| l.name == dp.name) {
                    let _ = apply_doc_command(
                        &mut tmp_doc,
                        DocCommand::SetLet {
                            name: dp.name.clone(),
                            value: val,
                            expr: None,
                        },
                    );
                }
            }
        }
        let let_vals: Vec<(String, f64)> = tmp_doc
            .lets
            .iter()
            .map(|l| (l.name.clone(), l.value))
            .collect();
        match CompiledSystem::compile(&self.spec, &let_vals) {
            Ok(c) => Cow::Owned(c),
            Err(_) => Cow::Borrowed(&self.compiled),
        }
    }

    /// Project `(state, design_p, epistemic_theta)` onto a cloned [`Document`], updating
    /// `system_block` nominals, matching `let`s, parametric sketches, and 3D `bind` BCs.
    pub fn synced_document(
        &self,
        state: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Result<Document, String> {
        let mut next_doc = self.base_doc.clone();
        sync_system_bindings_to_doc(&mut next_doc, state, design_p, epistemic_theta)?;
        Ok(next_doc)
    }

    /// Extract station-by-station field profiles and 3D boundary projection summaries for UI & MCP.
    pub fn extract_visual_fields(
        &self,
        state: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> DeclarativeSystemVisual {
        let comp = self.compiled_for_design(design_p);
        let nodes = comp.nodes;
        let n_fields = comp.n_fields;
        let fields = self
            .spec
            .states
            .iter()
            .enumerate()
            .map(|(f, st)| DeclarativeFieldProfile {
                name: st.name.clone(),
                symbol: if st.symbol.is_empty() {
                    st.name.clone()
                } else {
                    st.symbol.clone()
                },
                unit: st.unit.clone(),
                values: (0..nodes)
                    .map(|i| state.get(i * n_fields + f).copied().unwrap_or(0.0))
                    .collect(),
            })
            .collect();

        let bound_vals = comp.eval_bindings(state, design_p, epistemic_theta);
        let coord = self.spec.axis.to_spatial_coord();
        let bindings = self
            .spec
            .bindings
            .iter()
            .zip(bound_vals.iter())
            .map(|(b, vals)| {
                let mean = vals.iter().sum::<f64>() / (vals.len().max(1) as f64);
                let min = vals.iter().copied().fold(f64::INFINITY, f64::min);
                let max = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let lerp_expr = if nodes >= 2 && (max - min) > 1e-11 {
                    let samples: Vec<(f64, f64)> = comp
                        .stations
                        .iter()
                        .copied()
                        .zip(vals.iter().copied())
                        .collect();
                    SpatialFn::format_lerp(coord, &samples)
                } else {
                    None
                };
                DeclarativeBindingSummary {
                    target: b.target.clone(),
                    attr: b.attr.name().to_string(),
                    mean,
                    min,
                    max,
                    lerp_expr,
                }
            })
            .collect();

        DeclarativeSystemVisual {
            axis: self.spec.axis.name().to_string(),
            nodes,
            stations: comp.stations.clone(),
            fields,
            bindings,
        }
    }
}

impl CoupledEngineeringSystem for DeclarativeCoupledSystem {
    fn system_id(&self) -> &str {
        &self.spec.id
    }

    fn system_title(&self) -> &str {
        &self.title_display
    }

    fn industry_sector(&self) -> &str {
        &self.sector_display
    }

    fn field_descriptors(&self) -> Vec<FieldDescriptor> {
        let domain = if self.spec.nodes == 1 {
            "0D lumped node".to_string()
        } else {
            format!(
                "1D {}-axis [{} .. {} mm] ({} stations)",
                self.spec.axis.name(),
                self.spec.span[0],
                self.spec.span[1],
                self.spec.nodes
            )
        };
        self.spec
            .states
            .iter()
            .map(|st| FieldDescriptor {
                name: st.name.clone(),
                symbol: if st.symbol.is_empty() {
                    st.name.clone()
                } else {
                    st.symbol.clone()
                },
                unit: st.unit.clone(),
                reference_scale: st.scale,
                domain: domain.clone(),
            })
            .collect()
    }

    fn num_dofs(&self) -> usize {
        self.compiled.num_dofs()
    }

    fn design_parameters(&self) -> Vec<DesignParameter> {
        self.spec
            .designs
            .iter()
            .map(|d| {
                let span = (d.max - d.min).abs().max(1e-6);
                DesignParameter {
                    name: d.name.clone(),
                    symbol: if d.symbol.is_empty() {
                        d.name.clone()
                    } else {
                        d.symbol.clone()
                    },
                    unit: d.unit.clone(),
                    category: match d.category {
                        DeclDesignCategory::Geometry => DesignParameterCategory::GeometryContour,
                        DeclDesignCategory::Hardware => DesignParameterCategory::ComponentHardware,
                        DeclDesignCategory::Actuator => DesignParameterCategory::ActuatorSetpoint,
                        DeclDesignCategory::Process => DesignParameterCategory::ProcessAtmosphere,
                    },
                    nominal: d.nominal,
                    min_bound: d.min,
                    max_bound: d.max,
                    fd_step: (1e-5 * span).max(1e-7),
                    cost_rate_per_unit: d.cost_rate,
                    description: d.description.clone(),
                }
            })
            .collect()
    }

    fn epistemic_parameters(&self) -> Vec<EpistemicParameter> {
        self.spec
            .epistemics
            .iter()
            .map(|ep| {
                let span = (ep.max - ep.min).abs().max(1e-6);
                EpistemicParameter {
                    name: ep.name.clone(),
                    symbol: if ep.symbol.is_empty() {
                        ep.name.clone()
                    } else {
                        ep.symbol.clone()
                    },
                    unit: ep.unit.clone(),
                    nominal: ep.nominal,
                    std_dev_sigma: ep.sigma,
                    min_bound: ep.min,
                    max_bound: ep.max,
                    fd_step: (1e-5 * span).max(1e-7),
                    physical_mechanism: ep.mechanism.clone(),
                }
            })
            .collect()
    }

    fn objective_descriptors(&self) -> Vec<ObjectiveDescriptor> {
        self.spec
            .objectives
            .iter()
            .map(|obj| ObjectiveDescriptor {
                name: obj.name.clone(),
                symbol: if obj.symbol.is_empty() {
                    obj.name.clone()
                } else {
                    obj.symbol.clone()
                },
                unit: obj.unit.clone(),
                kind: match obj.kind {
                    DeclObjKind::Minimize => ObjectiveKind::Minimize,
                    DeclObjKind::Maximize => ObjectiveKind::Maximize,
                    DeclObjKind::Upper { limit } => ObjectiveKind::UpperLimit(limit),
                    DeclObjKind::Lower { limit } => ObjectiveKind::LowerLimit(limit),
                    DeclObjKind::Band { min, max } => ObjectiveKind::TargetBand { min, max },
                },
                description: obj.description.clone(),
            })
            .collect()
    }

    fn initial_state(&self, design_p: &[f64], epistemic_theta: &[f64]) -> Vec<f64> {
        self.compiled_for_design(design_p)
            .eval_initial_state(design_p, epistemic_theta)
    }

    fn eval_residual(
        &self,
        w: &[f64],
        w_prev: &[f64],
        inv_dt: f64,
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<f64> {
        self.compiled_for_design(design_p).eval_residual(
            w,
            w_prev,
            inv_dt,
            design_p,
            epistemic_theta,
        )
    }

    fn assemble_jacobian(
        &self,
        w: &[f64],
        w_prev: &[f64],
        inv_dt: f64,
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Result<(Vec<f64>, SparseJacobian, usize), String> {
        let comp = self.compiled_for_design(design_p);
        let ndof = comp.num_dofs();
        let (r0, triplets, cross_nnz) =
            comp.assemble_jacobian_triplets(w, w_prev, inv_dt, design_p, epistemic_theta);
        let j_mat = sparse_jacobian_from_triplets(ndof, ndof, &triplets)?;
        Ok((r0, j_mat, cross_nnz))
    }

    fn eval_objectives(&self, w: &[f64], design_p: &[f64], epistemic_theta: &[f64]) -> Vec<f64> {
        self.compiled_for_design(design_p)
            .eval_objectives(w, design_p, epistemic_theta)
    }

    fn eval_conservation_audits(
        &self,
        w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<ConservationAudit> {
        let comp = self.compiled_for_design(design_p);
        let tuples = comp.eval_audits(w, design_p, epistemic_theta);
        self.spec
            .audits
            .iter()
            .zip(tuples)
            .map(|(aud, (inflow, outflow, source_or_sink))| {
                let net_imbalance = inflow + source_or_sink - outflow;
                let ref_mag = inflow
                    .abs()
                    .max(outflow.abs())
                    .max(source_or_sink.abs())
                    .max(1e-9);
                let relative_error = net_imbalance.abs() / ref_mag;
                ConservationAudit {
                    name: aud.name.clone(),
                    inflow,
                    outflow,
                    source_or_sink,
                    net_imbalance,
                    unit: aud.unit.clone(),
                    relative_error,
                    tolerance: aud.tolerance,
                }
            })
            .collect()
    }

    fn eval_causal_attribution(
        &self,
        w: &[f64],
        design_p: &[f64],
        _epistemic_theta: &[f64],
    ) -> Vec<CausalAttributionTrack> {
        if self.spec.nodes < 2 || self.spec.states.is_empty() {
            return Vec::new();
        }
        let comp = self.compiled_for_design(design_p);
        let n_fields = comp.n_fields;
        let geom_mm = self
            .spec
            .designs
            .iter()
            .enumerate()
            .find(|(_, d)| d.category == DeclDesignCategory::Geometry)
            .and_then(|(k, _)| design_p.get(k).copied())
            .unwrap_or(1.0);
        let geom_label = self
            .spec
            .designs
            .iter()
            .find(|d| d.category == DeclDesignCategory::Geometry)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| "Geometry Lever".to_string());
        let st0 = &self.spec.states[0];
        let st1_idx = if n_fields >= 2 { 1 } else { 0 };
        let points = (0..comp.nodes)
            .map(|i| {
                let s_mm = comp.stations[i];
                let u0 = w.get(i * n_fields).copied().unwrap_or(0.0);
                let u1 = w.get(i * n_fields + st1_idx).copied().unwrap_or(u0);
                let u2 = if n_fields >= 3 {
                    w.get(i * n_fields + 2).copied().unwrap_or(0.0)
                } else {
                    0.0
                };
                CausalAttributionPoint {
                    coord_m: s_mm * 1e-3,
                    zone_label: format!("{}={:.1} mm", self.spec.axis.name(), s_mm),
                    geometry_profile_m: geom_mm * 1e-3,
                    primary_temperature_c: u0,
                    forward_transport_kw: u1,
                    return_or_loss_transport_kw: u2,
                    local_defect_birth: u0,
                    downstream_survival_factor: 1.0,
                    surviving_exit_defect: u0.abs(),
                }
            })
            .collect();
        vec![CausalAttributionTrack {
            track_name: format!("{} — 1D Station Profile", self.title_display),
            coord_unit: "m".to_string(),
            geometry_label: geom_label,
            defect_label: format!("|{}|", st0.name),
            defect_unit: st0.unit.clone(),
            points,
        }]
    }

    fn build_3d_fsm(&self, design_p: &[f64]) -> Option<String> {
        let th = self.nominal_epistemic_vector();
        let w0 = self.initial_state(design_p, &th);
        let synced = self.synced_document(&w0, design_p, &th).ok()?;
        Some(fsm::print(&synced))
    }

    fn build_3d_fsm_from_state(
        &self,
        state: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Option<String> {
        let synced = self
            .synced_document(state, design_p, epistemic_theta)
            .ok()?;
        Some(fsm::print(&synced))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convective_pipe_1d_matches_analytical_exponential_and_projects_onto_3d_thermo_elastic_fea() {
        // Analytical 1D plug-flow cooling/heating with constant wall temperature T_w = 120 C:
        //   mdot * cp * dT_f/ds = P_wet * h * (T_w - T_f),   T_f(0) = 20 C
        // Exact ODE solution: T_f(s) = T_w - (T_w - T_in) * exp(-s / L_th),
        // where L_th = mdot * cp / (P_wet * h).
        let fsm_src = "\
fieldsmith 1
name \"Analytical Convective Pipe Twin\"
material ss316l
let wall_t = 3
cylinder outer_pipe r=8 + wall_t h=100 at=(0, 0, 50)
cylinder inner_bore r=8 h=104 at=(0, 0, 50) subtract
region bore_wall = inner_bore.side
region outer_wall = outer_pipe.side
region pipe_base = outer_pipe.bottom
support base_fix pipe_base fixed=xyz
convection bore_conv bore_wall h=0.005 tinf=20 case=\"Thermal\"
convection outer_amb outer_wall h=0.0005 tinf=120 case=\"Thermal\"

system analytical_pipe title=\"1D Heated Pipe Plug-Flow\" sector=\"Thermal-Fluids\" nodes=41 span=(0, 100) axis=z {
  design mdot nominal=0.05 min=0.01 max=0.20 unit=\"kg/s\" symbol=\"m_dot\"
  design wall_t nominal=3 min=1.5 max=6 unit=\"mm\" symbol=\"t_w\" category=geometry
  epistemic h_scale nominal=1 sigma=0.1 min=0.7 max=1.3 unit=\"-\" symbol=\"c_h\"
  state T_f init=\"20\" scale=100 unit=\"C\" symbol=\"T_f\"
  aux P_wet expr=\"2 * pi * 8\"
  aux h_b expr=\"0.08 * h_scale\"
  residual T_f eq=\"1000 * mdot * d_ds(T_f) - P_wet * h_b * (120 - T_f)\" bc_left=\"T_f - 20\"
  objective exit_temp expr=\"T_f\" reduce=exit kind=maximize unit=\"C\" symbol=\"T_out\"
  objective mean_temp expr=\"T_f\" reduce=mean kind=upper limit=110 unit=\"C\" symbol=\"T_mean\"
  audit enthalpy_gain inflow=\"integral(P_wet * h_b * (120 - T_f))\" outflow=\"1000 * mdot * (exit(T_f) - entry(T_f))\" tol=0.03 unit=\"W\"
  bind bore_conv attr=tinf expr=\"T_f\"
  bind bore_conv attr=h expr=\"h_b * 0.1\"
}
solve size=6 tref=20 build=(0, 0)
";
        let doc = fsm::parse(fsm_src).expect("parse pipe .fsm");
        let sys = DeclarativeCoupledSystem::from_document(&doc).expect("compile system");
        let cfg = sys.engine_config();
        let p = sys.nominal_design_vector();
        let th = sys.nominal_epistemic_vector();
        let res = MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p, &th)
            .expect("solve declarative system");

        assert!(
            res.final_residual_l2 < 1e-10,
            "residual L2 = {:e}",
            res.final_residual_l2
        );
        assert!(
            res.directional_fd_jacobian_rel_err < 1e-6,
            "directional FD Jacobian err = {:e}",
            res.directional_fd_jacobian_rel_err
        );
        assert!(
            res.adjoint_vs_tangent_max_rel_err < 1e-6,
            "adjoint vs tangent err = {:e}",
            res.adjoint_vs_tangent_max_rel_err
        );
        assert!(
            res.conservation_audits[0].passed(),
            "conservation audit failed: {:?}",
            res.conservation_audits[0]
        );

        // Compare exit temperature at s = 100 against exact analytical exponential
        // N = 41 backward Euler (`(1 + ds/L_th)^(-40)` -> within ~0.3% of `exp(-100/L_th)`):
        let p_wet = 2.0 * std::f64::consts::PI * 8.0;
        let h_b = 0.08;
        let l_th = (1000.0 * 0.05) / (p_wet * h_b);
        let t_out_exact = 120.0 - 100.0 * (-100.0 / l_th).exp();
        let t_out_num = res.objectives[0];
        let rel_err = (t_out_num - t_out_exact).abs() / t_out_exact;
        assert!(
            rel_err < 0.015,
            "T_out numerical {t_out_num:.4} vs analytical {t_out_exact:.4} (rel_err={rel_err:.4})"
        );
        // Increasing mass flow `mdot` decreases residence time, so `d(T_out)/d(mdot) < 0`!
        assert!(
            res.design_sensitivities[0][0] < 0.0,
            "expected d(T_out)/d(mdot) < 0, got {}",
            res.design_sensitivities[0][0]
        );

        // Project onto 3D `.fsm` and run actual 3D P2 thermo-elastic FEA!
        let synced_fsm = sys
            .build_3d_fsm_from_state(&res.state, &p, &th)
            .expect("build synced 3D .fsm");
        let synced_doc = fsm::parse(&synced_fsm).expect("reparse synced 3D .fsm");
        assert!(synced_doc.system_block.is_some(), "system block preserved");
        assert!(synced_doc.convections[0]
            .t_inf_expr
            .as_deref()
            .is_some_and(|s| s.starts_with("lerp(z, 0, 20")));

        let fea = crate::analyse(&synced_doc, crate::Resolution::Size(6.0), crate::now_ms)
            .expect("3D P2 thermo-elastic FEA solve on projected 1D lerp BCs");
        assert_eq!(fea.cases.len(), 1);
        assert!(fea.cases[0].max_von_mises > 0.0);
        assert!(fea.cases[0].max_displacement > 0.0);
        let temps = fea.cases[0]
            .temperature
            .as_ref()
            .expect("thermal nodal temperatures");
        let t_min = temps.iter().copied().fold(f64::INFINITY, f64::min);
        let t_max = temps.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(
            t_max > t_min + 2.0,
            "expected axial 3D temperature gradient from projected lerp(z, ...), got [{t_min:.2} .. {t_max:.2}] C"
        );
    }
}
