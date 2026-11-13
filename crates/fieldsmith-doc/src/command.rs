//! Authoritative document mutation reducer (`DocCommand`) shared across MCP, WASM, and UI.
//!
//! Routing interactive UI edits and agent/API tool calls through [`apply_doc_command`] ensures
//! every surface applies identical validation, parameter propagation (`param::apply`), and
//! parametric sketch-feature synchronization (`sketch::sync_sketch_features`).

use crate::clinching::{apply_clinching_to_document, ClinchingApplyRequest, ClinchingApplySummary};
use crate::fixings::{
    insert_fixing_into_document, reconfigure_existing_bolt_stack, FixingInsertRequest,
    FixingInsertSummary, FixingReconfigureSpec, FixingStackInspection,
};
use crate::materials::Material;
use crate::model::*;
use crate::param::{self, ParamAddr};
use crate::sketch::{
    apply_sketch_feature_to_document, solve_sketch, sync_sketch_features, ParametricSketch,
    SketchFeatureRequest, SketchFeatureSummary, SketchSolveReport,
};
use serde::{Deserialize, Serialize};

/// Serializable document mutation command shared by `fieldsmith-mcp`, `fieldsmith-wasm`, and Web UI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum DocCommand {
    SetMaterial {
        material: String,
        #[serde(default)]
        node: Option<String>,
    },
    UpsertCustomMaterial {
        material: Material,
    },
    SetLet {
        name: String,
        value: f64,
        #[serde(default)]
        expr: Option<String>,
    },
    DeleteLet {
        name: String,
    },
    UpsertNode {
        node: Node,
        #[serde(default)]
        parent_group_id: Option<String>,
    },
    DeleteEntity {
        id: String,
    },
    UpsertRegion {
        #[serde(default)]
        id: Option<String>,
        selectors: Vec<Selector>,
    },
    UpsertSupport {
        #[serde(default)]
        id: Option<String>,
        region: String,
        fixed: [bool; 3],
    },
    UpsertContact {
        #[serde(default)]
        id: Option<String>,
        region: String,
        #[serde(default)]
        normal: Option<[f64; 3]>,
        #[serde(default)]
        k: Option<f64>,
        #[serde(default)]
        gap: Option<f64>,
        #[serde(default)]
        case: Option<String>,
    },
    UpsertLoad {
        #[serde(default)]
        id: Option<String>,
        region: String,
        #[serde(default)]
        case: Option<String>,
        kind: LoadKind,
    },
    UpsertTemperature {
        #[serde(default)]
        id: Option<String>,
        region: String,
        celsius: f64,
        #[serde(default)]
        expr: Option<String>,
        #[serde(default)]
        case: Option<String>,
    },
    UpsertConvection {
        #[serde(default)]
        id: Option<String>,
        region: String,
        h: f64,
        t_inf: f64,
        #[serde(default)]
        h_expr: Option<String>,
        #[serde(default)]
        t_inf_expr: Option<String>,
        #[serde(default)]
        case: Option<String>,
    },
    UpsertHeatFlux {
        #[serde(default)]
        id: Option<String>,
        region: String,
        q: f64,
        #[serde(default)]
        expr: Option<String>,
        #[serde(default)]
        case: Option<String>,
    },
    SetFill {
        fill: Option<Fill>,
    },
    SetHx {
        hx: Option<HxSpec>,
    },
    SetSolve {
        solve: SolveSettings,
    },
    UpsertSketch {
        sketch: ParametricSketch,
    },
    DeleteSketch {
        id: String,
    },
    ApplySketchFeature {
        request: SketchFeatureRequest,
    },
    SetSystemBlock {
        #[serde(default)]
        system: Option<crate::system::DeclarativeSystem>,
    },
    UpsertAssert {
        #[serde(default)]
        index: Option<usize>,
        assertion: crate::fsm::LiterateAssert,
    },
    DeleteAssert {
        index: usize,
    },
    UpsertUncertainty {
        #[serde(default)]
        index: Option<usize>,
        uncertainty: UncertaintySpec,
    },
    DeleteUncertainty {
        index: usize,
    },
    UpsertObserve {
        #[serde(default)]
        index: Option<usize>,
        observation: ObserveSpec,
    },
    DeleteObserve {
        index: usize,
    },
    InsertFixing {
        request: FixingInsertRequest,
    },
    ReconfigureFixing {
        request: FixingReconfigureSpec,
    },
    ApplyClinching {
        request: ClinchingApplyRequest,
    },
}

/// Outcome returned by [`apply_doc_command`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocCommandOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sketch_report: Option<SketchSolveReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sketch_feature: Option<SketchFeatureSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixing: Option<FixingInsertSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixing_stack: Option<FixingStackInspection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clinching: Option<ClinchingApplySummary>,
    pub problems: Vec<String>,
}

fn resolve_or_fresh_id(doc: &Document, id: Option<&str>, prefix: &str) -> String {
    match id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(explicit) => explicit.to_string(),
        None => doc.fresh_id(prefix),
    }
}

fn retain_removed<T>(items: &mut Vec<T>, keep: impl FnMut(&T) -> bool) -> bool {
    let before = items.len();
    items.retain(keep);
    items.len() < before
}

fn delete_node_recursive(nodes: &mut Vec<Node>, id: &str) -> bool {
    let before = nodes.len();
    nodes.retain(|n| n.id != id);
    if nodes.len() < before {
        return true;
    }
    for n in nodes.iter_mut() {
        if let NodeKind::Group { children, .. } = &mut n.kind {
            if delete_node_recursive(children, id) {
                return true;
            }
        }
    }
    false
}

fn delete_hole_companions_recursive(nodes: &mut Vec<Node>, feature_id: &str) {
    let prefix = format!("{feature_id}_hole");
    nodes.retain(|n| {
        if let Some(rest) = n.id.strip_prefix(&prefix) {
            if rest.parse::<usize>().is_ok() {
                return false;
            }
        }
        true
    });
    for n in nodes.iter_mut() {
        if let NodeKind::Group { children, .. } = &mut n.kind {
            delete_hole_companions_recursive(children, feature_id);
        }
    }
}

/// Apply one [`DocCommand`] to `doc`, maintaining parameter, consumer, and parametric sketch invariants.
pub fn apply_doc_command(doc: &mut Document, cmd: DocCommand) -> Result<DocCommandOutcome, String> {
    let mut entity_id: Option<String> = None;
    let entity_kind: Option<String>;
    let action: Option<String>;
    let mut sketch_report: Option<SketchSolveReport> = None;
    let mut sketch_feature: Option<SketchFeatureSummary> = None;
    let mut fixing: Option<FixingInsertSummary> = None;
    let mut fixing_stack: Option<FixingStackInspection> = None;
    let mut clinching: Option<ClinchingApplySummary> = None;

    match cmd {
        DocCommand::SetMaterial { material, node } => {
            let target = node.as_deref().map(str::trim).unwrap_or("");
            if target.is_empty() {
                doc.material = material.clone();
                entity_id = Some(material);
                entity_kind = Some("material".into());
                action = Some("updated".into());
            } else if doc.set_node_material(target, &material) {
                entity_id = Some(target.to_string());
                entity_kind = Some("node_material".into());
                action = Some("updated".into());
            } else {
                return Err(format!("set_material: no shape named {target:?}"));
            }
        }
        DocCommand::UpsertCustomMaterial { material } => {
            let id = material.id.clone();
            let act = if let Some(slot) = doc.custom_materials.iter().position(|m| m.id == id) {
                doc.custom_materials[slot] = material;
                "updated"
            } else {
                doc.custom_materials.push(material);
                "created"
            };
            entity_id = Some(id);
            entity_kind = Some("custom_material".into());
            action = Some(act.into());
        }
        DocCommand::SetLet { name, value, expr } => {
            if !value.is_finite() {
                return Err(format!("set_let: value {value} is not finite"));
            }
            let name = name.trim().to_string();
            if name.contains('.') {
                let addr = ParamAddr::parse(&name).map_err(|e| format!("set_let: {e}"))?;
                param::apply(doc, &addr, value).map_err(|e| format!("set_let: {e}"))?;
                entity_id = Some(name);
                entity_kind = Some("param".into());
                action = Some("updated".into());
            } else if let Some(slot) = doc.lets.iter().position(|l| l.name == name) {
                let effective_val = if let Some(ref ex) = expr {
                    let prior: Vec<(String, f64)> = doc.lets[..slot]
                        .iter()
                        .map(|l| (l.name.clone(), l.value))
                        .collect();
                    crate::fsm::eval_expr(ex, &prior).map_err(|m| format!("set_let: {m}"))?
                } else {
                    value
                };
                let addr = ParamAddr::parse(&name).map_err(|e| format!("set_let: {e}"))?;
                param::apply(doc, &addr, effective_val).map_err(|e| format!("set_let: {e}"))?;
                doc.lets[slot].expr = expr.unwrap_or_else(|| format!("{effective_val}"));
                entity_id = Some(name);
                entity_kind = Some("let".into());
                action = Some("updated".into());
            } else {
                if !crate::fsm::is_ident(&name) {
                    return Err(format!("set_let: {name:?} is not a valid let identifier"));
                }
                let effective_expr = expr.unwrap_or_else(|| format!("{value}"));
                let prior: Vec<(String, f64)> =
                    doc.lets.iter().map(|l| (l.name.clone(), l.value)).collect();
                let effective_val = crate::fsm::eval_expr(&effective_expr, &prior)
                    .map_err(|m| format!("set_let: {m}"))?;
                if !effective_val.is_finite() {
                    return Err(format!("set_let: value for {name:?} is not finite"));
                }
                doc.lets.push(Let {
                    name: name.clone(),
                    expr: effective_expr,
                    value: effective_val,
                });
                if !doc.sketch_features.is_empty() {
                    sync_sketch_features(doc)?;
                }
                entity_id = Some(name);
                entity_kind = Some("let".into());
                action = Some("created".into());
            }
        }
        DocCommand::DeleteLet { name } => {
            let before = doc.lets.len();
            doc.lets.retain(|l| l.name != name);
            if doc.lets.len() == before {
                return Err(format!("delete_let: no let named {name:?}"));
            }
            entity_id = Some(name);
            entity_kind = Some("let".into());
            action = Some("deleted".into());
        }
        DocCommand::UpsertNode {
            node,
            parent_group_id,
        } => {
            let id = node.id.clone();
            let act = if let Some(existing) = doc.find_mut(&id) {
                *existing = node;
                "updated"
            } else if let Some(gid) = parent_group_id.as_deref().filter(|s| !s.is_empty()) {
                let parent = doc
                    .find_mut(gid)
                    .ok_or_else(|| format!("upsert_node: parent group {gid:?} not found"))?;
                if let NodeKind::Group { children, .. } = &mut parent.kind {
                    children.push(node);
                    "created"
                } else {
                    return Err(format!("upsert_node: {gid:?} is not a group"));
                }
            } else {
                doc.root.push(node);
                "created"
            };
            if !doc.sketch_features.is_empty() {
                let _ = sync_sketch_features(doc);
            }
            entity_id = Some(id);
            entity_kind = Some("shape".into());
            action = Some(act.into());
        }
        DocCommand::DeleteEntity { id } => {
            if delete_node_recursive(&mut doc.root, &id) {
                let was_sketch_feat = doc.sketch_features.iter().any(|b| b.feature_id == id);
                doc.sketch_features.retain(|b| b.feature_id != id);
                if was_sketch_feat {
                    delete_hole_companions_recursive(&mut doc.root, &id);
                }
                entity_id = Some(id);
                entity_kind = Some("shape".into());
                action = Some("deleted".into());
            } else if retain_removed(&mut doc.sketches, |s| s.id != id) {
                doc.sketch_features.retain(|b| b.sketch_id != id);
                entity_id = Some(id);
                entity_kind = Some("sketch".into());
                action = Some("deleted".into());
            } else if retain_removed(&mut doc.regions, |r| r.id != id) {
                entity_id = Some(id);
                entity_kind = Some("region".into());
                action = Some("deleted".into());
            } else if retain_removed(&mut doc.supports, |s| s.id != id) {
                entity_id = Some(id);
                entity_kind = Some("support".into());
                action = Some("deleted".into());
            } else if retain_removed(&mut doc.contacts, |c| c.id != id) {
                entity_id = Some(id);
                entity_kind = Some("contact".into());
                action = Some("deleted".into());
            } else if retain_removed(&mut doc.loads, |l| l.id != id) {
                entity_id = Some(id);
                entity_kind = Some("load".into());
                action = Some("deleted".into());
            } else if retain_removed(&mut doc.temperatures, |t| t.id != id) {
                entity_id = Some(id);
                entity_kind = Some("temperature".into());
                action = Some("deleted".into());
            } else if retain_removed(&mut doc.convections, |c| c.id != id) {
                entity_id = Some(id);
                entity_kind = Some("convection".into());
                action = Some("deleted".into());
            } else if retain_removed(&mut doc.heat_fluxes, |hf| hf.id != id) {
                entity_id = Some(id);
                entity_kind = Some("heatflux".into());
                action = Some("deleted".into());
            } else if id == "fill" && doc.fill.is_some() {
                doc.fill = None;
                doc.hx = None;
                doc.consumed
                    .retain(|c| c.target != "fill" && c.target != "hx");
                entity_id = Some(id);
                entity_kind = Some("fill".into());
                action = Some("deleted".into());
            } else if id == "hx" && doc.hx.is_some() {
                doc.hx = None;
                doc.consumed.retain(|c| c.target != "hx");
                entity_id = Some(id);
                entity_kind = Some("hx".into());
                action = Some("deleted".into());
            } else {
                return Err(format!("delete_entity: no entity named {id:?}"));
            }
        }
        DocCommand::UpsertRegion { id, selectors } => {
            let rid = resolve_or_fresh_id(doc, id.as_deref(), "region");
            doc.regions.retain(|r| r.id != rid);
            doc.regions.push(Region {
                id: rid.clone(),
                selectors,
            });
            entity_id = Some(rid);
            entity_kind = Some("region".into());
            action = Some("upserted".into());
        }
        DocCommand::UpsertSupport { id, region, fixed } => {
            let sid = resolve_or_fresh_id(doc, id.as_deref(), "support");
            doc.supports.retain(|s| s.id != sid);
            doc.supports.push(Support {
                id: sid.clone(),
                region,
                fixed,
            });
            entity_id = Some(sid);
            entity_kind = Some("support".into());
            action = Some("upserted".into());
        }
        DocCommand::UpsertContact {
            id,
            region,
            normal,
            k,
            gap,
            case,
        } => {
            let cid = resolve_or_fresh_id(doc, id.as_deref(), "contact");
            let case = case
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_default();
            doc.contacts.retain(|c| c.id != cid);
            doc.contacts.push(Contact {
                id: cid.clone(),
                region,
                normal,
                k: k.unwrap_or(DEFAULT_CONTACT_K_MPA_MM),
                gap: gap.unwrap_or(0.0),
                case,
            });
            entity_id = Some(cid);
            entity_kind = Some("contact".into());
            action = Some("upserted".into());
        }
        DocCommand::UpsertLoad {
            id,
            region,
            case,
            kind,
        } => {
            let lid = resolve_or_fresh_id(doc, id.as_deref(), "load");
            let case = case
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Load 1".into());
            doc.loads.retain(|l| l.id != lid);
            doc.loads.push(Load {
                id: lid.clone(),
                region,
                case,
                kind,
            });
            entity_id = Some(lid);
            entity_kind = Some("load".into());
            action = Some("upserted".into());
        }
        DocCommand::UpsertTemperature {
            id,
            region,
            celsius,
            expr,
            case,
        } => {
            let tid = resolve_or_fresh_id(doc, id.as_deref(), "temperature");
            let case = case
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Thermal 1".into());
            doc.temperatures.retain(|t| t.id != tid);
            doc.temperatures.push(Temperature {
                id: tid.clone(),
                region,
                celsius,
                expr: expr.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
                case,
            });
            entity_id = Some(tid);
            entity_kind = Some("temperature".into());
            action = Some("upserted".into());
        }
        DocCommand::UpsertConvection {
            id,
            region,
            h,
            t_inf,
            h_expr,
            t_inf_expr,
            case,
        } => {
            let cid = resolve_or_fresh_id(doc, id.as_deref(), "conv");
            let case = case
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Thermal 1".into());
            doc.convections.retain(|c| c.id != cid);
            doc.convections.push(Convection {
                id: cid.clone(),
                region,
                h,
                t_inf,
                h_expr: h_expr
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                t_inf_expr: t_inf_expr
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                case,
            });
            entity_id = Some(cid);
            entity_kind = Some("convection".into());
            action = Some("upserted".into());
        }
        DocCommand::UpsertHeatFlux {
            id,
            region,
            q,
            expr,
            case,
        } => {
            let hfid = resolve_or_fresh_id(doc, id.as_deref(), "hf");
            let case = case
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Thermal 1".into());
            doc.heat_fluxes.retain(|hf| hf.id != hfid);
            doc.heat_fluxes.push(HeatFlux {
                id: hfid.clone(),
                region,
                q,
                expr: expr.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
                case,
            });
            entity_id = Some(hfid);
            entity_kind = Some("heatflux".into());
            action = Some("upserted".into());
        }
        DocCommand::SetFill { fill } => {
            doc.consumed.retain(|c| c.target != "fill");
            doc.fill = fill;
            entity_kind = Some("fill".into());
            action = Some("updated".into());
        }
        DocCommand::SetHx { hx } => {
            doc.consumed.retain(|c| c.target != "hx");
            doc.hx = hx;
            entity_kind = Some("hx".into());
            action = Some("updated".into());
        }
        DocCommand::SetSolve { solve } => {
            doc.solve = solve;
            entity_kind = Some("solve".into());
            action = Some("updated".into());
        }
        DocCommand::UpsertSketch { mut sketch } => {
            let sid = sketch.id.trim().to_string();
            if !crate::fsm::is_ident(&sid) {
                return Err(format!(
                    "upsert_sketch: '{sid}' is not a valid sketch identifier"
                ));
            }
            sketch.id = sid.clone();
            let let_pairs: Vec<(String, f64)> =
                doc.lets.iter().map(|l| (l.name.clone(), l.value)).collect();
            let rep = solve_sketch(&mut sketch, &let_pairs)?;
            let act = if let Some(slot) = doc.sketches.iter_mut().find(|s| s.id == sid) {
                *slot = rep.solved_sketch.clone();
                "updated"
            } else {
                doc.sketches.push(rep.solved_sketch.clone());
                "created"
            };
            if doc.sketch_features.iter().any(|b| b.sketch_id == sid) {
                sync_sketch_features(doc)?;
            }
            entity_id = Some(sid);
            entity_kind = Some("sketch".into());
            action = Some(act.into());
            sketch_report = Some(rep);
        }
        DocCommand::DeleteSketch { id } => {
            let before = doc.sketches.len();
            doc.sketches.retain(|s| s.id != id);
            if doc.sketches.len() == before {
                return Err(format!("delete_sketch: no sketch named {id:?}"));
            }
            doc.sketch_features.retain(|b| b.sketch_id != id);
            entity_id = Some(id);
            entity_kind = Some("sketch".into());
            action = Some("deleted".into());
        }
        DocCommand::ApplySketchFeature { request } => {
            let summary = apply_sketch_feature_to_document(doc, &request)?;
            entity_id = Some(summary.feature_id.clone());
            entity_kind = Some("sketch_feature".into());
            action = Some("applied".into());
            sketch_feature = Some(summary);
        }
        DocCommand::SetSystemBlock { system } => {
            if let Some(ref sys) = system {
                let errs = sys.validate(doc);
                if !errs.is_empty() {
                    return Err(format!("set_system_block: {}", errs.join("; ")));
                }
                entity_id = Some(sys.id.clone());
                action = Some("updated".into());
            } else {
                action = Some("deleted".into());
            }
            doc.system_block = system;
            entity_kind = Some("system".into());
        }
        DocCommand::UpsertAssert {
            index,
            mut assertion,
        } => {
            assertion.label = assertion.label.trim().to_string();
            assertion.metric = assertion.metric.trim().to_string();
            assertion.target = assertion.target.trim().to_string();
            assertion.tolerance = assertion.tolerance.trim().to_string();
            assertion.unit = assertion.unit.trim().to_string();
            if assertion.label.is_empty() {
                assertion.label = format!("{} {}", assertion.metric, assertion.target);
            }
            if assertion.metric.is_empty() {
                return Err("upsert_assert: metric cannot be empty".into());
            }
            if assertion.target.is_empty() {
                return Err("upsert_assert: target cannot be empty".into());
            }
            let lbl = assertion.label.clone();
            let act = if let Some(idx) = index {
                if idx < doc.asserts.len() {
                    doc.asserts[idx] = assertion;
                    "updated"
                } else {
                    return Err(format!(
                        "upsert_assert: index {idx} out of bounds (len {})",
                        doc.asserts.len()
                    ));
                }
            } else if let Some(slot) = doc.asserts.iter_mut().find(|a| a.label == lbl) {
                *slot = assertion;
                "updated"
            } else {
                doc.asserts.push(assertion);
                "created"
            };
            entity_id = Some(lbl);
            entity_kind = Some("assert".into());
            action = Some(act.into());
        }
        DocCommand::DeleteAssert { index } => {
            if index >= doc.asserts.len() {
                return Err(format!(
                    "delete_assert: index {index} out of bounds (len {})",
                    doc.asserts.len()
                ));
            }
            let removed = doc.asserts.remove(index);
            entity_id = Some(removed.label);
            entity_kind = Some("assert".into());
            action = Some("deleted".into());
        }
        DocCommand::UpsertUncertainty {
            index,
            mut uncertainty,
        } => {
            uncertainty.label = uncertainty.label.trim().to_string();
            uncertainty.param = uncertainty.param.trim().to_string();
            uncertainty.unit = uncertainty.unit.trim().to_string();
            if uncertainty.param.is_empty() {
                return Err("upsert_uncertainty: param cannot be empty".into());
            }
            ParamAddr::parse(&uncertainty.param).map_err(|m| format!("upsert_uncertainty: {m}"))?;
            if let Some(s) = uncertainty.std {
                if !s.is_finite() || s < 0.0 {
                    return Err("upsert_uncertainty: std must be finite and >= 0".into());
                }
            }
            if let Some(c) = uncertainty.cv {
                if !c.is_finite() || c < 0.0 {
                    return Err("upsert_uncertainty: cv must be finite and >= 0".into());
                }
            }
            if uncertainty.std.unwrap_or(0.0) <= 0.0 && uncertainty.cv.unwrap_or(0.0) <= 0.0 {
                return Err("upsert_uncertainty: specify std > 0 and/or cv > 0".into());
            }
            if uncertainty.label.is_empty() {
                uncertainty.label = uncertainty.param.clone();
            }
            let lbl = uncertainty.label.clone();
            let act = if let Some(idx) = index {
                if idx < doc.uncertainties.len() {
                    doc.uncertainties[idx] = uncertainty;
                    "updated"
                } else {
                    return Err(format!(
                        "upsert_uncertainty: index {idx} out of bounds (len {})",
                        doc.uncertainties.len()
                    ));
                }
            } else if let Some(slot) = doc
                .uncertainties
                .iter_mut()
                .find(|u| u.param == uncertainty.param && u.label == lbl)
            {
                *slot = uncertainty;
                "updated"
            } else {
                doc.uncertainties.push(uncertainty);
                "created"
            };
            entity_id = Some(lbl);
            entity_kind = Some("uncertainty".into());
            action = Some(act.into());
        }
        DocCommand::DeleteUncertainty { index } => {
            if index >= doc.uncertainties.len() {
                return Err(format!(
                    "delete_uncertainty: index {index} out of bounds (len {})",
                    doc.uncertainties.len()
                ));
            }
            let removed = doc.uncertainties.remove(index);
            entity_id = Some(removed.label);
            entity_kind = Some("uncertainty".into());
            action = Some("deleted".into());
        }
        DocCommand::UpsertObserve {
            index,
            mut observation,
        } => {
            observation.label = observation.label.trim().to_string();
            observation.metric = observation.metric.trim().to_string();
            observation.unit = observation.unit.trim().to_string();
            if observation.metric.is_empty() {
                return Err("upsert_observe: metric cannot be empty".into());
            }
            if !observation.measured.is_finite() {
                return Err("upsert_observe: measured must be finite".into());
            }
            if !observation.std.is_finite() || observation.std <= 0.0 {
                return Err("upsert_observe: std must be finite and > 0".into());
            }
            if observation.label.is_empty() {
                observation.label = observation.metric.clone();
            }
            let lbl = observation.label.clone();
            let act = if let Some(idx) = index {
                if idx < doc.observations.len() {
                    doc.observations[idx] = observation;
                    "updated"
                } else {
                    return Err(format!(
                        "upsert_observe: index {idx} out of bounds (len {})",
                        doc.observations.len()
                    ));
                }
            } else if let Some(slot) = doc
                .observations
                .iter_mut()
                .find(|o| o.metric == observation.metric && o.label == lbl)
            {
                *slot = observation;
                "updated"
            } else {
                doc.observations.push(observation);
                "created"
            };
            entity_id = Some(lbl);
            entity_kind = Some("observe".into());
            action = Some(act.into());
        }
        DocCommand::DeleteObserve { index } => {
            if index >= doc.observations.len() {
                return Err(format!(
                    "delete_observe: index {index} out of bounds (len {})",
                    doc.observations.len()
                ));
            }
            let removed = doc.observations.remove(index);
            entity_id = Some(removed.label);
            entity_kind = Some("observe".into());
            action = Some("deleted".into());
        }
        DocCommand::InsertFixing { request } => {
            let summary = insert_fixing_into_document(doc, &request);
            entity_id = Some(summary.first_new_id.clone());
            entity_kind = Some("fixing".into());
            action = Some("inserted".into());
            fixing = Some(summary);
        }
        DocCommand::ReconfigureFixing { request } => {
            let Some(stack) = reconfigure_existing_bolt_stack(doc, &request) else {
                return Err(format!(
                    "reconfigure_fixing: node {:?} is not an inspectable bolt or screw stack",
                    request.selected_node_id
                ));
            };
            entity_id = Some(stack.hw_group_id.clone());
            entity_kind = Some("fixing".into());
            action = Some("updated".into());
            fixing_stack = Some(stack);
        }
        DocCommand::ApplyClinching { request } => {
            let summary = apply_clinching_to_document(doc, &request)?;
            entity_id = summary
                .inserted_nodes
                .first()
                .cloned()
                .or_else(|| Some("X".into()));
            entity_kind = Some("clinching".into());
            action = Some(if request.insert_joint {
                "inserted".into()
            } else {
                "updated".into()
            });
            clinching = Some(summary);
        }
    }

    Ok(DocCommandOutcome {
        entity_id,
        entity_kind,
        action,
        sketch_report,
        sketch_feature,
        fixing,
        fixing_stack,
        clinching,
        problems: doc.validate(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsm;
    use crate::sketch::{
        SketchConstraint, SketchConstraintKind, SketchFeatureOp, SketchPlane, SketchPoint,
        SketchSegment,
    };
    use std::collections::BTreeMap;

    #[test]
    fn sketch_and_sketchfeature_round_trip_in_fsm_and_react_to_let_edits() {
        let mut doc = fsm::parse(
            "fieldsmith 1\n\
             name \"Parametric Boss Assembly\"\n\
             material ss316l\n\
             let plate_t = 12\n\
             let arm_w = 40\n\
             let bore_d = 10\n\
             box base_plate size=(100, 80, plate_t) at=(0, 0, plate_t / 2)\n",
        )
        .expect("parse base");

        let mut sk = ParametricSketch {
            id: "arm_boss_sk".into(),
            plane: SketchPlane::OnFace {
                host_id: "base_plate".into(),
                face: Face::Top,
                offset: 0.0,
            },
            points: vec![
                SketchPoint {
                    id: "p0".into(),
                    x: -20.0,
                    y: -15.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "p1".into(),
                    x: 20.0,
                    y: -15.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "p2".into(),
                    x: 20.0,
                    y: 15.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "p3".into(),
                    x: -20.0,
                    y: 15.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "hc".into(),
                    x: 0.0,
                    y: 0.0,
                    fixed: false,
                    construction: true,
                },
            ],
            segments: vec![
                SketchSegment {
                    id: "s0".into(),
                    p1: "p0".into(),
                    p2: "p1".into(),
                    bulge: 0.0,
                    construction: false,
                },
                SketchSegment {
                    id: "s1".into(),
                    p1: "p1".into(),
                    p2: "p2".into(),
                    bulge: 0.0,
                    construction: false,
                },
                SketchSegment {
                    id: "s2".into(),
                    p1: "p2".into(),
                    p2: "p3".into(),
                    bulge: 0.0,
                    construction: false,
                },
                SketchSegment {
                    id: "s3".into(),
                    p1: "p3".into(),
                    p2: "p0".into(),
                    bulge: 0.0,
                    construction: false,
                },
            ],
            circles: vec![crate::sketch::SketchCircle {
                id: "center_bore".into(),
                center: "hc".into(),
                radius: 5.0,
                construction: false,
            }],
            constraints: vec![
                SketchConstraint {
                    id: "fix_p0".into(),
                    kind: SketchConstraintKind::Fixed {
                        point: "p0".into(),
                        x: -20.0,
                        y: -15.0,
                    },
                },
                SketchConstraint {
                    id: "fix_hc".into(),
                    kind: SketchConstraintKind::Fixed {
                        point: "hc".into(),
                        x: 0.0,
                        y: 0.0,
                    },
                },
                SketchConstraint {
                    id: "h0".into(),
                    kind: SketchConstraintKind::Horizontal {
                        segment: Some("s0".into()),
                        p1: None,
                        p2: None,
                    },
                },
                SketchConstraint {
                    id: "h2".into(),
                    kind: SketchConstraintKind::Horizontal {
                        segment: Some("s2".into()),
                        p1: None,
                        p2: None,
                    },
                },
                SketchConstraint {
                    id: "v1".into(),
                    kind: SketchConstraintKind::Vertical {
                        segment: Some("s1".into()),
                        p1: None,
                        p2: None,
                    },
                },
                SketchConstraint {
                    id: "v3".into(),
                    kind: SketchConstraintKind::Vertical {
                        segment: Some("s3".into()),
                        p1: None,
                        p2: None,
                    },
                },
                SketchConstraint {
                    id: "w_dim".into(),
                    kind: SketchConstraintKind::Distance {
                        segment: Some("s0".into()),
                        p1: None,
                        p2: None,
                        value: 40.0,
                        expr: Some("arm_w".into()),
                    },
                },
                SketchConstraint {
                    id: "h_dim".into(),
                    kind: SketchConstraintKind::Distance {
                        segment: Some("s1".into()),
                        p1: None,
                        p2: None,
                        value: 30.0,
                        expr: None,
                    },
                },
                SketchConstraint {
                    id: "bore_dim".into(),
                    kind: SketchConstraintKind::Diameter {
                        target: "center_bore".into(),
                        value: 10.0,
                        expr: Some("bore_d".into()),
                    },
                },
            ],
            macros: vec![],
        };
        // Ensure zero DOF before applying
        let init_rep = solve_sketch(
            &mut sk,
            &[
                ("plate_t".into(), 12.0),
                ("arm_w".into(), 40.0),
                ("bore_d".into(), 10.0),
            ],
        )
        .unwrap();
        assert_eq!(init_rep.dof, 0);

        let out = apply_doc_command(
            &mut doc,
            DocCommand::ApplySketchFeature {
                request: SketchFeatureRequest {
                    feature_id: "arm_boss".into(),
                    sketch_id: None,
                    sketch: sk,
                    preset: None,
                    on: None,
                    operation: SketchFeatureOp::BossExtrude {
                        height: 10.0,
                        blend: 1.0,
                        symmetric: false,
                    },
                    associate_with_host: false,
                    lets: BTreeMap::new(),
                },
            },
        )
        .expect("apply sketch feature");
        assert!(out.problems.is_empty(), "problems: {:?}", out.problems);
        assert_eq!(doc.sketches.len(), 1);
        assert_eq!(doc.sketch_features.len(), 1);
        assert_eq!(doc.sketches[0].id, "arm_boss_sk");

        // 1. Canonical `.fsm` round-trip preserves `sketch` and `sketchfeature` exactly!
        let canon = fsm::print(&doc);
        assert!(canon.contains("sketch arm_boss_sk on=\"base_plate.top\" {"));
        assert!(canon
            .contains("sketchfeature arm_boss sketch=arm_boss_sk kind=boss_extrude h=10 blend=1"));
        let mut reparsed = fsm::parse(&canon).expect("reparse canonical fsm");
        assert_eq!(reparsed, doc);
        assert_eq!(fsm::print(&reparsed), canon);

        // 2. Mutating `let arm_w = 65` AND `let plate_t = 20` AND `let bore_d = 16` via `DocCommand::SetLet`
        //    automatically re-solves `arm_boss_sk` AND updates `arm_boss` + `arm_boss_hole1`!
        apply_doc_command(
            &mut reparsed,
            DocCommand::SetLet {
                name: "arm_w".into(),
                value: 65.0,
                expr: None,
            },
        )
        .unwrap();
        apply_doc_command(
            &mut reparsed,
            DocCommand::SetLet {
                name: "plate_t".into(),
                value: 20.0,
                expr: None,
            },
        )
        .unwrap();
        apply_doc_command(
            &mut reparsed,
            DocCommand::SetLet {
                name: "bore_d".into(),
                value: 16.0,
                expr: None,
            },
        )
        .unwrap();

        let boss = reparsed.find("arm_boss").expect("arm_boss node");
        // `plate_t = 20` -> `base_plate.top` is now at z = 20, so a 10 mm boss sits at z = 25!
        assert!(
            (boss.transform.translate[2] - 25.0).abs() < 1e-6,
            "expected z=25.0 after plate_t=20, got {:?}",
            boss.transform.translate
        );
        // `arm_w = 65` anchored at p0.x = -20 -> right edge is now at x = +45!
        let NodeKind::Extrude { profile, .. } = &boss.kind else {
            panic!("expected Extrude");
        };
        let max_x = profile
            .iter()
            .map(|v| v.x)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (max_x - 45.0).abs() < 1e-4,
            "expected max_x = 45.0 after arm_w=65, got {max_x}"
        );
        // `bore_d = 16` -> radius = 8 -> hole profile spans x in [-8, +8]!
        let hole = reparsed.find("arm_boss_hole1").expect("arm_boss_hole1");
        let NodeKind::Extrude {
            profile: hole_prof, ..
        } = &hole.kind
        else {
            panic!("expected hole Extrude");
        };
        let hole_r = hole_prof.iter().map(|v| v.x.abs()).fold(0.0_f64, f64::max);
        assert!(
            (hole_r - 8.0).abs() < 1e-4,
            "expected hole radius 8.0 after bore_d=16, got {hole_r}"
        );
    }

    #[test]
    fn hand_authored_fsm_sketchfeature_materializes_3d_nodes_on_parse() {
        let src = r#"fieldsmith 1
name "Hand Authored Sketch Part"
material ss316l
let link_len = 50

sketch slot_sk {
  macro slot id=body p1=(0, 0) p2=(50, 0) r=10
  macro circle id=pin1 at=(0, 0) r=4
}
sketchfeature link_arm sketch=slot_sk kind=boss_extrude h=8
region top_face = link_arm.top
support fix1 top_face fixed=xyz
"#;
        let doc = fsm::parse(src).expect("parse hand-authored sketchfeature");
        assert!(doc.validate().is_empty(), "{:?}", doc.validate());
        assert!(doc.find("link_arm").is_some());
        assert!(doc.find("link_arm_hole1").is_some());
    }

    #[test]
    fn upsert_and_delete_assert_round_trip_through_fsm() {
        let mut doc = fsm::parse(
            "fieldsmith 1\n\
             name \"Assert Command Test\"\n\
             material ss316l\n\
             box b size=(20, 10, 10)\n",
        )
        .expect("parse");
        let out = apply_doc_command(
            &mut doc,
            DocCommand::UpsertAssert {
                index: None,
                assertion: fsm::LiterateAssert {
                    label: "Peak stress cap".into(),
                    metric: "max_vm_elem".into(),
                    target: "<= 180".into(),
                    tolerance: String::new(),
                    unit: "MPa".into(),
                },
            },
        )
        .expect("upsert assert");
        assert_eq!(out.action.as_deref(), Some("created"));
        assert_eq!(doc.asserts.len(), 1);
        let printed = fsm::print(&doc);
        assert!(printed.contains(
            "assert \"Peak stress cap\" metric=\"max_vm_elem\" target=\"<= 180\" unit=\"MPa\""
        ));
        let reparsed = fsm::parse(&printed).expect("reparse");
        assert_eq!(reparsed.asserts, doc.asserts);

        let del = apply_doc_command(&mut doc, DocCommand::DeleteAssert { index: 0 })
            .expect("delete assert");
        assert_eq!(del.action.as_deref(), Some("deleted"));
        assert!(doc.asserts.is_empty());
    }

    #[test]
    fn upsert_and_delete_uncertainty_round_trip_through_fsm() {
        let mut doc = fsm::parse(
            "fieldsmith 1\n\
             name \"UQ Command Test\"\n\
             material ss316l\n\
             let d = 10\n\
             box b size=(20, d, 10)\n",
        )
        .expect("parse");
        let out = apply_doc_command(
            &mut doc,
            DocCommand::UpsertUncertainty {
                index: None,
                uncertainty: UncertaintySpec {
                    label: "Depth tolerance".into(),
                    param: "d".into(),
                    std: Some(0.12),
                    cv: None,
                    unit: "mm".into(),
                },
            },
        )
        .expect("upsert uncertainty");
        assert_eq!(out.action.as_deref(), Some("created"));
        assert_eq!(doc.uncertainties.len(), 1);
        let printed = fsm::print(&doc);
        assert!(
            printed.contains("uncertainty \"Depth tolerance\" param=\"d\" std=0.12 unit=\"mm\"")
        );
        let reparsed = fsm::parse(&printed).expect("reparse");
        assert_eq!(reparsed.uncertainties, doc.uncertainties);

        let del = apply_doc_command(&mut doc, DocCommand::DeleteUncertainty { index: 0 })
            .expect("delete uncertainty");
        assert_eq!(del.action.as_deref(), Some("deleted"));
        assert!(doc.uncertainties.is_empty());
    }

    #[test]
    fn insert_and_reconfigure_fixing_round_trip_through_fsm() {
        use crate::fixings::{AssociatedNutKind, FixingKind, FixingPlacementMode};
        let mut doc = fsm::parse(
            "fieldsmith 1\n\
             name \"Bolted Flange Test\"\n\
             material al7075\n\
             box flange size=(80, 60, 12) at=(0, 0, 6)\n",
        )
        .expect("parse flange");

        let out = apply_doc_command(
            &mut doc,
            DocCommand::InsertFixing {
                request: FixingInsertRequest {
                    kind: FixingKind::HexCapBolt,
                    size: "M5".into(),
                    engagement_mm: 10.0,
                    host_id: Some("flange".into()),
                    host_face: Some(Face::Top),
                    placement_mode: Some(FixingPlacementMode::Flange4Corners),
                    flange_inset_x_mm: Some(12.0),
                    flange_inset_y_mm: Some(10.0),
                    include_washer: true,
                    associated_nut: AssociatedNutKind::NylocNut,
                    preload_cv: Some(0.15),
                    fastener_material: "steel_10_9".into(),
                    ..Default::default()
                },
            },
        )
        .expect("insert_fixing command");

        assert!(out.problems.is_empty(), "problems: {:?}", out.problems);
        assert_eq!(out.action.as_deref(), Some("inserted"));
        assert_eq!(out.entity_kind.as_deref(), Some("fixing"));
        let summary = out.fixing.expect("fixing summary");
        assert_eq!(summary.loads_added.len(), 4);
        assert_eq!(doc.uncertainties.len(), 1);
        assert_eq!(doc.uncertainties[0].cv, Some(0.15));

        let reconf = apply_doc_command(
            &mut doc,
            DocCommand::ReconfigureFixing {
                request: FixingReconfigureSpec {
                    selected_node_id: summary.first_new_id.clone(),
                    include_washer: false,
                    associated_nut: AssociatedNutKind::HexNut,
                    fastener_material: Some("ti64".into()),
                },
            },
        )
        .expect("reconfigure_fixing command");
        assert!(
            reconf.problems.is_empty(),
            "problems: {:?}",
            reconf.problems
        );
        let stack = reconf.fixing_stack.expect("fixing stack");
        assert!(!stack.include_washer);
        assert_eq!(stack.associated_nut, AssociatedNutKind::HexNut);
        assert_eq!(stack.fastener_material, "ti64");

        let canon = fsm::print(&doc);
        let reparsed = fsm::parse(&canon).expect("reparse fixing fsm");
        assert_eq!(reparsed, doc);
        assert_eq!(fsm::print(&reparsed), canon);
    }

    #[test]
    fn apply_clinching_stamps_host_sheet_and_round_trips_through_fsm() {
        let mut doc = fsm::parse(
            "fieldsmith 1\n\
             name \"DC04 Lap Sheet\"\n\
             material mat_dc04\n\
             box sheet size=(40, 26, 3.0) at=(0, 0, 1.5)\n\
             region clamp_end = sheet.nx\n\
             support fix clamp_end fixed=xyz\n",
        )
        .expect("parse DC04 sheet");

        let ins = apply_doc_command(
            &mut doc,
            DocCommand::ApplyClinching {
                request: ClinchingApplyRequest {
                    insert_joint: true,
                    host_id: Some("sheet".into()),
                    dial_optimal_x: true,
                    j2_plasticity: Some(true),
                    bottom_x_std_mm: Some(0.04),
                    ..Default::default()
                },
            },
        )
        .expect("apply_clinching on host sheet");
        assert!(ins.problems.is_empty(), "problems: {:?}", ins.problems);
        assert_eq!(ins.action.as_deref(), Some("inserted"));
        let sum = ins.clinching.expect("clinching summary");
        assert!(sum.j2_active);
        assert!((sum.applied_x_mm - sum.optimal_x_mm).abs() < 1e-9);

        let canon = fsm::print(&doc);
        let reparsed = fsm::parse(&canon).expect("reparse clinched sheet");
        assert_eq!(reparsed, doc);
        assert_eq!(fsm::print(&reparsed), canon);
    }

    #[test]
    fn upsert_contact_and_let_driven_contact_round_trip_through_fsm() {
        let mut doc = fsm::parse(
            "fieldsmith 1\n\
             name \"Flange Seat\"\n\
             material ti64\n\
             let k_bed = 25000\n\
             let g0 = 0.05\n\
             box flange size=(60, 40, 8) at=(30, 20, 4)\n\
             region bed = flange.nz\n\
             region bore = flange.nx\n\
             support pin bore fixed=xy\n\
             contact seat bed normal=(0, 0, -1) k=k_bed gap=g0\n",
        )
        .expect("parse contact fsm");
        assert!(doc.validate().is_empty(), "validate: {:?}", doc.validate());
        assert_eq!(doc.contacts.len(), 1);
        assert_eq!(doc.contacts[0].k, 25000.0);
        assert_eq!(doc.contacts[0].gap, 0.05);
        assert_eq!(doc.contacts[0].normal, Some([0.0, 0.0, -1.0]));

        // Sweeping let `g0` updates `doc.contacts[0].gap` and preserves canonical `.fsm`.
        apply_doc_command(
            &mut doc,
            DocCommand::SetLet {
                name: "g0".into(),
                value: 0.12,
                expr: None,
            },
        )
        .expect("set_let g0");
        assert!((doc.contacts[0].gap - 0.12).abs() < 1e-12);
        let canon = fsm::print(&doc);
        assert!(canon.contains("contact seat bed normal=(0, 0, -1) k=k_bed gap=g0"));
        let reparsed = fsm::parse(&canon).expect("reparse contact fsm");
        assert_eq!(reparsed, doc);

        // Upsert + delete via DocCommand.
        let out = apply_doc_command(
            &mut doc,
            DocCommand::UpsertContact {
                id: None,
                region: "bed".into(),
                normal: None,
                k: Some(15000.0),
                gap: Some(0.0),
                case: Some("Load 1".into()),
            },
        )
        .expect("upsert_contact");
        let cid = out.entity_id.expect("contact id");
        assert_eq!(cid, "contact1");
        assert_eq!(doc.contacts.len(), 2);

        apply_doc_command(&mut doc, DocCommand::DeleteEntity { id: cid }).expect("delete contact1");
        assert_eq!(doc.contacts.len(), 1);
    }

    #[test]
    fn tpms_hx_round_trips_and_reacts_to_let_edits() {
        let mut doc = fsm::parse(
            "fieldsmith 1\n\
             name \"Gyroid Recuperator\"\n\
             material alsi10mg\n\
             let m_hot = 0.04\n\
             let t_in = 90\n\
             box core size=(60, 20, 20) at=(30, 10, 10)\n\
             region port_in = core.nx\n\
             region port_out = core.px\n\
             support mount port_in fixed=xyz\n\
             load press port_out pressure=0.5\n\
             fill gyroid sheet cell=6 skin=0 rho=(0.25, 0.25) curve=1 wall=0.4 margin=1\n\
             hx inlet=port_in outlet=port_out mdot=m_hot tin=t_in fluid=water arrangement=counterflow cold_mdot=m_hot cold_tin=20 pout=105\n\
             solve\n",
        )
        .expect("parse hx fsm");
        assert!(doc.validate().is_empty(), "validate: {:?}", doc.validate());
        let hx = doc.hx.as_ref().expect("hx parsed");
        assert_eq!(hx.inlet, "port_in");
        assert_eq!(hx.outlet, "port_out");
        assert!((hx.mdot - 0.04).abs() < 1e-12);
        assert!((hx.tin - 90.0).abs() < 1e-12);
        assert_eq!(hx.fluid, HxFluid::Water);
        assert_eq!(hx.arrangement, HxArrangement::Counterflow);
        assert_eq!(hx.cold_mdot, Some(0.04));
        assert!((hx.cold_tin - 20.0).abs() < 1e-12);
        assert!((hx.pout_kpa - 105.0).abs() < 1e-12);

        apply_doc_command(
            &mut doc,
            DocCommand::SetLet {
                name: "m_hot".into(),
                value: 0.065,
                expr: None,
            },
        )
        .expect("set_let m_hot");
        assert!((doc.hx.as_ref().unwrap().mdot - 0.065).abs() < 1e-12);
        assert_eq!(doc.hx.as_ref().unwrap().cold_mdot, Some(0.065));

        let canon = fsm::print(&doc);
        assert!(
            canon.contains("hx inlet=port_in outlet=port_out mdot=m_hot tin=t_in fluid=water arrangement=counterflow cold_mdot=m_hot cold_tin=20 pout=105"),
            "canon: {canon}"
        );
        let reparsed = fsm::parse(&canon).expect("reparse hx fsm");
        assert_eq!(reparsed, doc);

        apply_doc_command(&mut doc, DocCommand::DeleteEntity { id: "hx".into() })
            .expect("delete hx");
        assert!(doc.hx.is_none());
    }

    #[test]
    fn upsert_and_delete_observe_commands_round_trip() {
        let mut doc = fsm::parse(
            "fieldsmith 1\n\
             name \"Observe Cmd\"\n\
             material petg\n\
             box b size=(20, 10, 5) at=(10, 5, 2.5)\n\
             solve\n",
        )
        .expect("parse");
        let out = apply_doc_command(
            &mut doc,
            DocCommand::UpsertObserve {
                index: None,
                observation: ObserveSpec {
                    label: "Bench tip gauge".into(),
                    metric: "max_displacement".into(),
                    measured: 0.85,
                    std: 0.02,
                    unit: "mm".into(),
                },
            },
        )
        .expect("upsert observe");
        assert_eq!(out.action.as_deref(), Some("created"));
        assert_eq!(doc.observations.len(), 1);

        let out2 = apply_doc_command(
            &mut doc,
            DocCommand::UpsertObserve {
                index: Some(0),
                observation: ObserveSpec {
                    label: "Bench tip gauge".into(),
                    metric: "max_displacement".into(),
                    measured: 0.81,
                    std: 0.015,
                    unit: "mm".into(),
                },
            },
        )
        .expect("update observe");
        assert_eq!(out2.action.as_deref(), Some("updated"));
        assert!((doc.observations[0].measured - 0.81).abs() < 1e-12);

        apply_doc_command(&mut doc, DocCommand::DeleteObserve { index: 0 })
            .expect("delete observe");
        assert!(doc.observations.is_empty());
    }
}
