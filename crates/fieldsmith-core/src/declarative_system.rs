//! Bridge from a `.fsm` [`DeclarativeSystem`] (`system <id> { ... }`) to the universal
//! [`CoupledEngineeringSystem`] / [`MonolithicSystemEngine`] solver, 3D SDF & graded-TPMS
//! cross-section extractor (`section`), Sparse Adjoint & Tangent sensitivity analyser,
//! FOSM `p10/p50/p90` uncertainty propagator, and 3D `.fsm` BC projector.

use crate::porous_hx::{local_porous_props, tpms_cell_law};
use fieldsmith_doc::command::{apply_doc_command, DocCommand};
use fieldsmith_doc::fsm;
use fieldsmith_doc::spatial::SpatialFn;
use fieldsmith_doc::system::{
    sync_system_bindings_to_doc_exact_with_sections, sync_system_bindings_to_doc_with_sections,
    CompiledSystem, DeclAxis, DeclDesignCategory, DeclObjKind, DeclSectionMode, DeclSectionProp,
    DeclarativeSystem,
};
use fieldsmith_doc::{Document, FillMode};
use fieldsmith_field::expr::{self, Expr};
use fieldsmith_field::lattice::calibration;
use fieldsmith_mesh::slice::{polygon_signed_area, slice_field};
use fieldsmith_solve::coupled_system::{
    sparse_jacobian_from_triplets, CausalAttributionPoint, CausalAttributionTrack,
    ConservationAudit, CoupledEngineeringSystem, DesignParameter, DesignParameterCategory,
    EngineConfig, EpistemicParameter, FieldDescriptor, MonolithicSystemEngine, NamedScenario,
    ObjectiveDescriptor, ObjectiveKind, SparseJacobian,
};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::sync::{Arc, Mutex};

/// Locked transverse marching-squares grid domain for one `section` declaration so central
/// finite-difference perturbations `p_k ± δ` evaluate zero crossings along the exact same
/// world-space grid edges without step-count or origin jitter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LockedSliceDomain {
    pub u_ax: usize,
    pub v_ax: usize,
    pub s_ax: usize,
    pub lo_perm: [f64; 3],
    pub hi_perm: [f64; 3],
    pub slice_h: f64,
    pub s_cap_lo: f64,
    pub s_cap_hi: f64,
    pub cap_inset: f64,
}

fn axis_perm(axis: DeclAxis) -> Result<(usize, usize, usize), String> {
    match axis {
        DeclAxis::X => Ok((1, 2, 0)),
        DeclAxis::Y => Ok((2, 0, 1)),
        DeclAxis::Z => Ok((0, 1, 2)),
        DeclAxis::R => Err("section axis cannot be radial 'r' (expected x, y, or z)".into()),
    }
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

#[inline]
fn smooth_max(a: f64, b: f64, eps: f64) -> f64 {
    let d = a - b;
    0.5 * (a + b + (d * d + eps * eps).sqrt())
}

#[inline]
fn min_dist_to_polygons_2d(pt: [f64; 2], polys: &[Vec<[f64; 2]>]) -> f64 {
    let mut best_sq = f64::INFINITY;
    for poly in polys {
        let n = poly.len();
        if n < 2 {
            continue;
        }
        for k in 0..n {
            let a = poly[k];
            let b = poly[(k + 1) % n];
            let abx = b[0] - a[0];
            let aby = b[1] - a[1];
            let len_sq = abx * abx + aby * aby;
            let t = if len_sq > 1e-24 {
                (((pt[0] - a[0]) * abx + (pt[1] - a[1]) * aby) / len_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let dx = pt[0] - (a[0] + t * abx);
            let dy = pt[1] - (a[1] + t * aby);
            let d2 = dx * dx + dy * dy;
            if d2 < best_sq {
                best_sq = d2;
            }
        }
    }
    best_sq.sqrt()
}

/// Lock transverse slicing domains for every `section` in `spec` from reference document `ref_doc`.
pub fn lock_slice_domains(
    ref_doc: &Document,
    spec: &DeclarativeSystem,
) -> Result<Vec<LockedSliceDomain>, String> {
    if spec.sections.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::with_capacity(spec.sections.len());
    for sec in &spec.sections {
        let eff_axis = sec.axis.unwrap_or(spec.axis);
        let (u_ax, v_ax, s_ax) = axis_perm(eff_axis)?;
        let compiled_target = if let Some(ref nid) = sec.shape {
            expr::placed_shape(ref_doc, nid)
                .ok_or_else(|| format!("section '{}' references missing shape '{nid}'", sec.name))?
        } else {
            expr::compile_body(ref_doc)
        };
        let b = expr::bounds(&compiled_target);
        if b.is_empty() {
            return Err(format!(
                "section '{}': compiled 3D SDF bounds are empty",
                sec.name
            ));
        }
        let span_u = (b.hi[u_ax] - b.lo[u_ax]).max(1e-3);
        let span_v = (b.hi[v_ax] - b.lo[v_ax]).max(1e-3);
        let max_tv = span_u.max(span_v);
        let slice_h = if let Some(h_explicit) = sec.h {
            h_explicit.max(1e-3)
        } else {
            let h_auto = (max_tv / 120.0).clamp(0.02, 1.0);
            2.0f64.powf(h_auto.log2().floor()).max(1e-3)
        };
        let pad = 0.25 * max_tv + 4.0 * slice_h;
        let u0 = ((b.lo[u_ax] - pad) / slice_h).floor() * slice_h;
        let u1 = ((b.hi[u_ax] + pad) / slice_h).ceil() * slice_h;
        let v0 = ((b.lo[v_ax] - pad) / slice_h).floor() * slice_h;
        let v1 = ((b.hi[v_ax] + pad) / slice_h).ceil() * slice_h;
        let s_cap_lo = b.lo[s_ax];
        let s_cap_hi = b.hi[s_ax];
        let s_full = (s_cap_hi - s_cap_lo).max(1e-6);
        let cap_inset = (1e-4 * s_full).clamp(1e-5, 0.05);
        out.push(LockedSliceDomain {
            u_ax,
            v_ax,
            s_ax,
            lo_perm: [u0, v0, s_cap_lo - slice_h],
            hi_perm: [u1, v1, s_cap_hi + slice_h],
            slice_h,
            s_cap_lo,
            s_cap_hi,
            cap_inset,
        });
    }
    Ok(out)
}

/// Extract the flattened `nodes * spec.sections.len()` grid of 3D SDF and graded-TPMS section
/// properties along `spec`'s stations.
pub fn extract_system_section_grid(
    doc: &Document,
    spec: &DeclarativeSystem,
    locked_domains: Option<&[LockedSliceDomain]>,
) -> Result<Vec<f64>, String> {
    let n_sec = spec.sections.len();
    if n_sec == 0 {
        return Ok(Vec::new());
    }
    let nodes = spec.nodes.max(1);
    let span_len = (spec.span[1] - spec.span[0]).abs();
    let ds = if nodes >= 2 {
        (spec.span[1] - spec.span[0]) / ((nodes - 1) as f64)
    } else {
        span_len.max(1.0)
    };
    let stations: Vec<f64> = if nodes >= 2 {
        (0..nodes).map(|i| spec.span[0] + (i as f64) * ds).collect()
    } else {
        vec![spec.span[0]]
    };

    let owned_domains;
    let domains: &[LockedSliceDomain] = match locked_domains {
        Some(d) if d.len() == n_sec => d,
        _ => {
            owned_domains = lock_slice_domains(doc, spec)?;
            &owned_domains
        }
    };

    let keep_expr = if let Some(ref fill_spec) = doc.fill {
        crate::fill::keep_zones(doc, fill_spec).map_err(|e| e.0)?
    } else {
        Expr::Empty
    };
    let has_keep = !matches!(keep_expr, Expr::Empty);

    // Group sections sharing `(shape, domain)` so marching squares runs once per unique target.
    let mut groups: Vec<(Option<String>, LockedSliceDomain, Vec<usize>)> = Vec::new();
    for (sec_idx, sec) in spec.sections.iter().enumerate() {
        let dom = domains[sec_idx];
        if let Some(g) = groups
            .iter_mut()
            .find(|(sh, d, _)| *sh == sec.shape && *d == dom)
        {
            g.2.push(sec_idx);
        } else {
            groups.push((sec.shape.clone(), dom, vec![sec_idx]));
        }
    }

    let s_span_lo = spec.span[0].min(spec.span[1]);
    let s_span_hi = spec.span[0].max(spec.span[1]);
    let mut grid = vec![0.0; nodes * n_sec];

    for (shape_opt, dom, member_idxs) in groups {
        let compiled_target = if let Some(ref nid) = shape_opt {
            expr::placed_shape(doc, nid)
                .ok_or_else(|| format!("section shape '{nid}' not found in document"))?
        } else {
            expr::compile_body(doc)
        };
        let needs_fill_core = member_idxs.iter().any(|&idx| {
            matches!(
                spec.sections[idx].prop,
                DeclSectionProp::FillArea
                    | DeclSectionProp::FillRho
                    | DeclSectionProp::FillPorosity
                    | DeclSectionProp::FillDh
                    | DeclSectionProp::FillPerm
                    | DeclSectionProp::FillBeta
                    | DeclSectionProp::FillAwet
            )
        });
        let map_pt = |uvs: [f64; 3]| -> [f64; 3] {
            let mut world = [0.0; 3];
            world[dom.u_ax] = uvs[0];
            world[dom.v_ax] = uvs[1];
            world[dom.s_ax] = uvs[2];
            world
        };

        for (i, &s_raw) in stations.iter().enumerate() {
            let s_eff = if dom.s_cap_hi - dom.s_cap_lo > 2.0 * dom.cap_inset {
                s_raw.clamp(dom.s_cap_lo + dom.cap_inset, dom.s_cap_hi - dom.cap_inset)
            } else {
                0.5 * (dom.s_cap_lo + dom.s_cap_hi)
            };
            let layer = slice_field(
                |uvs| expr::eval(&compiled_target, map_pt(uvs)).v,
                dom.lo_perm,
                dom.hi_perm,
                s_eff,
                dom.slice_h,
            );

            let mut void_area = 0.0f64;
            let mut void_perim = 0.0f64;
            let mut void_count = 0usize;
            let mut outer_area = 0.0f64;
            let mut outer_perim = 0.0f64;
            let mut outer_count = 0usize;
            for poly in &layer.polygons {
                let sa = polygon_signed_area(poly);
                let per = polygon_perimeter(poly);
                if sa < -1e-9 {
                    void_area += -sa;
                    void_perim += per;
                    void_count += 1;
                } else if sa > 1e-9 {
                    outer_area += sa;
                    outer_perim += per;
                    outer_count += 1;
                }
            }
            let solid_net_area = (outer_area - void_area).max(0.0);
            let solid_net_perim = outer_perim + void_perim;

            // Evaluate graded-TPMS porous core properties if requested by any section in this group.
            let (
                core_area_mm2,
                fill_rho,
                fill_porosity,
                fill_cell_mm,
                fill_dh_mm,
                fill_perm_mm2,
                fill_beta_per_mm,
                fill_awet_per_mm,
            ) = if needs_fill_core {
                if let Some(ref fill_spec) = doc.fill {
                    let t_grade = if (s_span_hi - s_span_lo).abs() > 1e-9 {
                        ((s_raw - s_span_lo) / (s_span_hi - s_span_lo)).clamp(0.0, 1.0)
                    } else {
                        0.5
                    };
                    let g_curved = t_grade.powf(fill_spec.curve.max(1e-3));
                    let sk = fill_spec.skin
                        + g_curved
                            * (fill_spec.skin_max.unwrap_or(fill_spec.skin) - fill_spec.skin);
                    let erode = fill_spec.erode.unwrap_or(0.0).max(0.0) * (1.0 - g_curved);
                    let inset = sk + erode;
                    let core_area = if solid_net_area <= 1e-12 || layer.polygons.is_empty() {
                        0.0
                    } else if inset.abs() <= 1e-12 && !has_keep {
                        solid_net_area
                    } else {
                        let band_hi = 2.0 * dom.slice_h;
                        let core_layer = slice_field(
                            |uvs| {
                                let p = map_pt(uvs);
                                let d3 = expr::eval(&compiled_target, p).v;
                                let d_in = if d3 >= band_hi {
                                    d3 + inset
                                } else {
                                    let sgn = if d3 < 0.0 { -1.0 } else { 1.0 };
                                    sgn * min_dist_to_polygons_2d([uvs[0], uvs[1]], &layer.polygons)
                                        + inset
                                };
                                if has_keep {
                                    d_in.max(-expr::eval(&keep_expr, p).v)
                                } else {
                                    d_in
                                }
                            },
                            dom.lo_perm,
                            dom.hi_perm,
                            s_eff,
                            dom.slice_h,
                        );
                        core_layer.area().max(0.0)
                    };

                    let a_mm = (fill_spec.cell
                        + g_curved
                            * (fill_spec.cell_min.unwrap_or(fill_spec.cell) - fill_spec.cell))
                        .max(1e-3);
                    let rho_target =
                        fill_spec.rho_min + g_curved * (fill_spec.rho_max - fill_spec.rho_min);
                    let cal_lo = calibration(fill_spec.family, fill_spec.mode);
                    let law_lo = tpms_cell_law(fill_spec.family);
                    let w0_raw = cal_lo.param_for(rho_target, a_mm);
                    let w0 = if fill_spec.mode == FillMode::Sheet {
                        smooth_max(w0_raw, fill_spec.min_wall, 1e-4)
                    } else {
                        w0_raw
                    };
                    let (rho_rev, law, w_eff) =
                        if let Some(mf) = fill_spec.morph.filter(|&m| m != fill_spec.family) {
                            let cal_hi = calibration(mf, fill_spec.mode);
                            let law_hi = tpms_cell_law(mf);
                            let w1_raw = cal_hi.param_for(rho_target, a_mm);
                            let w1 = if fill_spec.mode == FillMode::Sheet {
                                smooth_max(w1_raw, fill_spec.min_wall, 1e-4)
                            } else {
                                w1_raw
                            };
                            let r0 = cal_lo.rho_for(w0, a_mm);
                            let r1 = cal_hi.rho_for(w1, a_mm);
                            (
                                (1.0 - g_curved) * r0 + g_curved * r1,
                                law_lo.lerp(law_hi, g_curved),
                                (1.0 - g_curved) * w0 + g_curved * w1,
                            )
                        } else {
                            (cal_lo.rho_for(w0, a_mm), law_lo, w0)
                        };
                    let props = local_porous_props(law, fill_spec.mode, rho_rev, a_mm, w_eff);
                    if core_area > 1e-9 {
                        (
                            core_area,
                            1.0 - props.porosity,
                            props.porosity,
                            a_mm,
                            props.hydraulic_diam_m * 1e3,
                            props.permeability_m2 * 1e6,
                            props.beta_per_m * 1e-3,
                            props.wetted_area_per_m * 1e-3,
                        )
                    } else {
                        (
                            0.0,
                            if solid_net_area > 1e-9 { 1.0 } else { 0.0 },
                            0.0,
                            a_mm,
                            0.0,
                            0.0,
                            0.0,
                            0.0,
                        )
                    }
                } else {
                    (
                        0.0,
                        if solid_net_area > 1e-9 { 1.0 } else { 0.0 },
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                    )
                }
            } else {
                let a_mm = doc
                    .fill
                    .as_ref()
                    .map(|fs| {
                        let t_grade = if (s_span_hi - s_span_lo).abs() > 1e-9 {
                            ((s_raw - s_span_lo) / (s_span_hi - s_span_lo)).clamp(0.0, 1.0)
                        } else {
                            0.5
                        };
                        let g_curved = t_grade.powf(fs.curve.max(1e-3));
                        (fs.cell + g_curved * (fs.cell_min.unwrap_or(fs.cell) - fs.cell)).max(1e-3)
                    })
                    .unwrap_or(0.0);
                (0.0, 0.0, 0.0, a_mm, 0.0, 0.0, 0.0, 0.0)
            };

            for &sec_idx in &member_idxs {
                let sec = &spec.sections[sec_idx];
                let (mode_area, mode_perim, mode_cnt) = match sec.mode {
                    DeclSectionMode::Void => (void_area, void_perim, void_count),
                    DeclSectionMode::Outer => (outer_area, outer_perim, outer_count),
                    DeclSectionMode::Solid => (solid_net_area, solid_net_perim, outer_count),
                    DeclSectionMode::Auto => {
                        if sec.shape.is_none() && void_count > 0 {
                            (void_area, void_perim, void_count)
                        } else {
                            (solid_net_area, solid_net_perim, outer_count)
                        }
                    }
                };
                let val = match sec.prop {
                    DeclSectionProp::Area => mode_area,
                    DeclSectionProp::Perimeter => mode_perim,
                    DeclSectionProp::Dh => {
                        if mode_perim > 1e-9 {
                            4.0 * mode_area / mode_perim
                        } else {
                            0.0
                        }
                    }
                    DeclSectionProp::Count => mode_cnt as f64,
                    DeclSectionProp::FillArea => core_area_mm2,
                    DeclSectionProp::FillRho => fill_rho,
                    DeclSectionProp::FillPorosity => fill_porosity,
                    DeclSectionProp::FillCell => fill_cell_mm,
                    DeclSectionProp::FillDh => fill_dh_mm,
                    DeclSectionProp::FillPerm => fill_perm_mm2,
                    DeclSectionProp::FillBeta => fill_beta_per_mm,
                    DeclSectionProp::FillAwet => fill_awet_per_mm,
                };
                grid[sec_idx * nodes + i] = val;
            }
        }
    }

    Ok(grid)
}

/// If `doc.system_block` is present and declares `bind` projections, solve its monolithic 0D/1D
/// equilibrium `R(W; p, θ) = 0` (propagating any matching top-level `doc.lets` values into
/// `design_p` and extracting any 3D `section` profiles) and return an owned [`Document`] with full
/// IEEE-754 `lerp(...)` spatial boundary conditions and bound `let` parameters projected in-memory.
/// Returns `Cow::Borrowed(doc)` with zero allocation when `doc.system_block` is `None`.
pub fn resolve_system_doc(doc: &Document) -> Result<Cow<'_, Document>, crate::SetupError> {
    resolve_system_doc_inner(doc, None)
}

/// Like [`resolve_system_doc`], but locks transverse 2D marching-squares slicing domains for any
/// `section` declarations to `ref_doc` so central finite-difference 3D sensitivity solves
/// (`p_k ± δ`) evaluate `3D SDF → 1D section → 1D system → 3D FEA` on an invariant grid.
pub fn resolve_system_doc_with_ref<'a>(
    doc: &'a Document,
    ref_doc: &Document,
) -> Result<Cow<'a, Document>, crate::SetupError> {
    resolve_system_doc_inner(doc, Some(ref_doc))
}

fn resolve_system_doc_inner<'a>(
    doc: &'a Document,
    ref_doc: Option<&Document>,
) -> Result<Cow<'a, Document>, crate::SetupError> {
    let Some(ref sb) = doc.system_block else {
        return Ok(Cow::Borrowed(doc));
    };
    if sb.bindings.is_empty() {
        return Ok(Cow::Borrowed(doc));
    }
    let sys = DeclarativeCoupledSystem::from_document_with_ref(doc, ref_doc)
        .map_err(crate::SetupError)?;
    let design_p = sys.nominal_design_vector();
    let theta = sys.nominal_epistemic_vector();
    let cfg = sys.engine_config();
    let (state, _, _) = MonolithicSystemEngine::solve_equilibrium(&sys, &cfg, &design_p, &theta)
        .map_err(crate::SetupError)?;
    let mut next_doc = doc.clone();
    let locked = sys.locked_domains.clone();
    sync_system_bindings_to_doc_exact_with_sections(
        &mut next_doc,
        &state,
        &design_p,
        &theta,
        |d, s| extract_system_section_grid(d, s, Some(&locked)),
    )
    .map_err(crate::SetupError)?;
    Ok(Cow::Owned(next_doc))
}

/// Synchronise an embedded `doc.system_block` equilibrium onto `doc` using compact (`.6`) `.fsm`
/// `lerp(...)` formatting so printed `.fsm` source stays concise and self-consistent.
pub fn sync_system_doc_compact(doc: &mut Document) -> Result<(), crate::SetupError> {
    let Some(ref sb) = doc.system_block else {
        return Ok(());
    };
    let sys = DeclarativeCoupledSystem::from_document(doc).map_err(crate::SetupError)?;
    let design_p = sys.nominal_design_vector();
    let theta = sys.nominal_epistemic_vector();
    let cfg = sys.engine_config();
    let (state, _, _) = MonolithicSystemEngine::solve_equilibrium(&sys, &cfg, &design_p, &theta)
        .map_err(crate::SetupError)?;
    if !sb.bindings.is_empty() {
        let locked = sys.locked_domains.clone();
        sync_system_bindings_to_doc_with_sections(doc, &state, &design_p, &theta, |d, s| {
            extract_system_section_grid(d, s, Some(&locked))
        })
        .map_err(crate::SetupError)?;
    } else if let Some(ref mut sb_mut) = doc.system_block {
        for (k, &pv) in design_p.iter().enumerate() {
            sb_mut.designs[k].nominal = pv;
        }
    }
    Ok(())
}

/// Solve `doc.system_block` once and return both the projected [`Document`] (when `bindings` is
/// non-empty, else `Cow::Borrowed(doc)`) and `Some(DeclarativeSystemVisual)`. Returns
/// `(Cow::Borrowed(doc), None)` with zero allocation when `doc.system_block` is `None`.
pub fn resolve_system_doc_and_visual(
    doc: &Document,
) -> Result<(Cow<'_, Document>, Option<DeclarativeSystemVisual>), crate::SetupError> {
    let Some(ref sb) = doc.system_block else {
        return Ok((Cow::Borrowed(doc), None));
    };
    let sys = DeclarativeCoupledSystem::from_document(doc).map_err(crate::SetupError)?;
    let design_p = sys.nominal_design_vector();
    let theta = sys.nominal_epistemic_vector();
    let cfg = sys.engine_config();
    let (state, _, _) = MonolithicSystemEngine::solve_equilibrium(&sys, &cfg, &design_p, &theta)
        .map_err(crate::SetupError)?;
    let vis = sys.extract_visual_fields(&state, &design_p, &theta);
    if sb.bindings.is_empty() {
        return Ok((Cow::Borrowed(doc), Some(vis)));
    }
    let mut next_doc = doc.clone();
    let locked = sys.locked_domains.clone();
    sync_system_bindings_to_doc_exact_with_sections(
        &mut next_doc,
        &state,
        &design_p,
        &theta,
        |d, s| extract_system_section_grid(d, s, Some(&locked)),
    )
    .map_err(crate::SetupError)?;
    Ok((Cow::Owned(next_doc), Some(vis)))
}

/// Solve `doc.system_block` at the current document state and return [`DeclarativeSystemVisual`]
/// (station profiles, 3D `section` summaries, converged `objectives`, and 3D `bind` summaries).
pub fn solve_declarative_system_visual(
    doc: &Document,
) -> Result<DeclarativeSystemVisual, crate::SetupError> {
    solve_declarative_system_visual_with_ref(doc, None)
}

fn solve_declarative_system_visual_with_ref(
    doc: &Document,
    ref_doc: Option<&Document>,
) -> Result<DeclarativeSystemVisual, crate::SetupError> {
    let sys = DeclarativeCoupledSystem::from_document_with_ref(doc, ref_doc)
        .map_err(crate::SetupError)?;
    let design_p = sys.nominal_design_vector();
    let theta = sys.nominal_epistemic_vector();
    let cfg = sys.engine_config();
    let (state, _, _) = MonolithicSystemEngine::solve_equilibrium(&sys, &cfg, &design_p, &theta)
        .map_err(crate::SetupError)?;
    Ok(sys.extract_visual_fields(&state, &design_p, &theta))
}

/// Resolve a `sys.<objective>` / `sys_<objective>` or `section.<name>.<min|max|mean|entry|exit>`
/// metric from a [`DeclarativeSystemVisual`]. Returns `None` if `metric` is not a system/section
/// metric.
pub fn resolve_system_visual_metric(
    vis: &DeclarativeSystemVisual,
    metric: &str,
) -> Option<Result<f64, String>> {
    let m = metric.trim();
    if let Some(obj_name) = m.strip_prefix("sys.").or_else(|| m.strip_prefix("sys_")) {
        let obj_name = obj_name.trim();
        return Some(
            vis.objectives
                .iter()
                .find(|o| o.name == obj_name)
                .map(|o| o.value)
                .ok_or_else(|| {
                    let avail: Vec<&str> = vis.objectives.iter().map(|o| o.name.as_str()).collect();
                    format!(
                        "metric '{m}': unknown system objective '{obj_name}' (available: [{}])",
                        avail.join(", ")
                    )
                }),
        );
    }
    if let Some(rest) = m.strip_prefix("section.") {
        let (sec_name, red) = match rest.split_once('.') {
            Some((sn, r)) => (sn.trim(), r.trim()),
            None => (rest.trim(), "mean"),
        };
        let Some(sec) = vis.sections.iter().find(|s| s.name == sec_name) else {
            let avail: Vec<&str> = vis.sections.iter().map(|s| s.name.as_str()).collect();
            return Some(Err(format!(
                "metric '{m}': unknown system section '{sec_name}' (available: [{}])",
                avail.join(", ")
            )));
        };
        let val = match red {
            "min" => Ok(sec.min),
            "max" => Ok(sec.max),
            "mean" | "avg" | "" => Ok(sec.mean),
            "entry" | "inlet" => Ok(sec.entry),
            "exit" | "outlet" => Ok(sec.exit),
            other => Err(format!(
                "metric '{m}': unknown section reduction '{other}' (expected min, max, mean, entry, or exit)"
            )),
        };
        return Some(val);
    }
    None
}

/// Evaluate `∂M / ∂p` for a `sys.*` / `section.*` metric via locked-grid central finite differences
/// on the 1D declarative system (`0` 3D FEA solves). Returns `None` if `metric` is not a
/// `sys.*` / `section.*` metric.
pub fn system_metric_derivative_for_param(
    doc: &Document,
    metric: &str,
    pname: &str,
    p0: f64,
) -> Option<Result<f64, crate::SetupError>> {
    let m = metric.trim();
    if !(m.starts_with("sys.") || m.starts_with("sys_") || m.starts_with("section.")) {
        return None;
    }
    let run = || -> Result<f64, crate::SetupError> {
        let addr = fieldsmith_doc::param::ParamAddr::parse(pname).map_err(crate::SetupError)?;
        let delta = (1e-4 * p0.abs()).max(1e-6);
        let eval_at = |val: f64| -> Result<f64, crate::SetupError> {
            let mut d_pert = doc.clone();
            crate::sensitivity::apply_sens_param(&mut d_pert, &addr, val)
                .map_err(crate::SetupError)?;
            let vis = solve_declarative_system_visual_with_ref(&d_pert, Some(doc))?;
            resolve_system_visual_metric(&vis, m)
                .unwrap_or_else(|| Err(format!("metric '{m}' is not a system/section metric")))
                .map_err(crate::SetupError)
        };
        match (eval_at(p0 + delta), eval_at(p0 - delta)) {
            (Ok(vh), Ok(vl)) => Ok((vh - vl) / (2.0 * delta)),
            (Ok(vh), Err(_)) => {
                let v0 = eval_at(p0)?;
                Ok((vh - v0) / delta)
            }
            (Err(_), Ok(vl)) => {
                let v0 = eval_at(p0)?;
                Ok((v0 - vl) / delta)
            }
            (Err(e), Err(_)) => Err(e),
        }
    };
    Some(run())
}

/// Solved 0D/1D station profiles, extracted 3D `section` profiles, converged objectives, and 3D BC
/// projection preview returned alongside [`fieldsmith_solve::coupled_system::SystemAnalysisResult`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeclarativeSystemVisual {
    pub axis: String,
    pub nodes: usize,
    pub stations: Vec<f64>,
    pub fields: Vec<DeclarativeFieldProfile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<DeclarativeSectionSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objectives: Vec<DeclarativeObjectiveSummary>,
    pub bindings: Vec<DeclarativeBindingSummary>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeclarativeFieldProfile {
    pub name: String,
    pub symbol: String,
    pub unit: String,
    pub values: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeclarativeSectionSummary {
    pub name: String,
    pub prop: String,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    pub unit: String,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub entry: f64,
    pub exit: f64,
    pub values: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeclarativeObjectiveSummary {
    pub name: String,
    pub symbol: String,
    pub unit: String,
    pub kind: String,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeclarativeBindingSummary {
    pub target: String,
    pub attr: String,
    pub mean: f64,
    pub min: f64,
    pub max: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lerp_expr: Option<String>,
}

type DesignCompileCache = Arc<Mutex<Option<(Vec<f64>, Arc<CompiledSystem>)>>>;

/// [`CoupledEngineeringSystem`] backed by a user-authored `.fsm` [`DeclarativeSystem`].
#[derive(Clone, Debug)]
pub struct DeclarativeCoupledSystem {
    base_doc: Document,
    spec: DeclarativeSystem,
    compiled: CompiledSystem,
    locked_domains: Vec<LockedSliceDomain>,
    base_design_p: Vec<f64>,
    has_design_linked_lets: bool,
    design_cache: DesignCompileCache,
    title_display: String,
    sector_display: String,
}

impl DeclarativeCoupledSystem {
    /// Construct and compile a [`DeclarativeCoupledSystem`] from `doc` (`doc.system_block` must be `Some`).
    pub fn from_document(doc: &Document) -> Result<Self, String> {
        Self::from_document_with_ref(doc, None)
    }

    /// Construct and compile a [`DeclarativeCoupledSystem`] from `doc`, locking any 3D `section`
    /// slicing grids to `ref_doc` when provided.
    pub fn from_document_with_ref(
        doc: &Document,
        ref_doc: Option<&Document>,
    ) -> Result<Self, String> {
        let mut spec = doc
            .system_block
            .clone()
            .ok_or_else(|| "document has no declarative 'system' block".to_string())?;
        // Propagate live top-level `doc.lets` values into matching `design` nominals (widening
        // bounds if a parameter sweep / sensitivity step stepped outside the initial box).
        for dp in &mut spec.designs {
            if let Some(l) = doc.lets.iter().find(|l| l.name == dp.name) {
                if l.value.is_finite() {
                    dp.nominal = l.value;
                    let span = (dp.max - dp.min).abs().max(1e-3);
                    if dp.nominal < dp.min {
                        dp.min = dp.nominal - 0.1 * span;
                    }
                    if dp.nominal > dp.max {
                        dp.max = dp.nominal + 0.1 * span;
                    }
                }
            }
        }
        let val_errs = spec.validate(doc);
        if !val_errs.is_empty() {
            return Err(val_errs.join("; "));
        }
        let domain_doc = ref_doc.unwrap_or(doc);
        let locked_domains = lock_slice_domains(domain_doc, &spec)?;
        let section_grid = extract_system_section_grid(doc, &spec, Some(&locked_domains))?;
        let let_vals: Vec<(String, f64)> =
            doc.lets.iter().map(|l| (l.name.clone(), l.value)).collect();
        let compiled = CompiledSystem::compile_with_sections(&spec, &let_vals, section_grid)
            .map_err(|errs| errs.join("; "))?;
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
            locked_domains,
            base_design_p,
            has_design_linked_lets,
            design_cache: Arc::new(Mutex::new(None)),
            title_display,
            sector_display,
        })
    }

    pub fn spec(&self) -> &DeclarativeSystem {
        &self.spec
    }

    pub fn locked_domains(&self) -> &[LockedSliceDomain] {
        &self.locked_domains
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

    /// Return a compiled system reflecting `design_p` (re-evaluating chained `doc.lets`, 3D
    /// shapes/sketches/fills, and 3D `section` cross-sections on `self.locked_domains` if any
    /// `design` lever shares its name with a `let` in the parent document).
    fn compiled_for_design(&self, design_p: &[f64]) -> Cow<'_, CompiledSystem> {
        if !self.has_design_linked_lets || design_p == self.base_design_p.as_slice() {
            return Cow::Borrowed(&self.compiled);
        }
        if let Ok(guard) = self.design_cache.lock() {
            if let Some((ref cached_p, ref cached_comp)) = *guard {
                if cached_p.as_slice() == design_p {
                    return Cow::Owned((**cached_comp).clone());
                }
            }
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
        let section_grid =
            match extract_system_section_grid(&tmp_doc, &self.spec, Some(&self.locked_domains)) {
                Ok(g) => g,
                Err(_) => return Cow::Borrowed(&self.compiled),
            };
        let let_vals: Vec<(String, f64)> = tmp_doc
            .lets
            .iter()
            .map(|l| (l.name.clone(), l.value))
            .collect();
        match CompiledSystem::compile_with_sections(&self.spec, &let_vals, section_grid) {
            Ok(c) => {
                if let Ok(mut guard) = self.design_cache.lock() {
                    *guard = Some((design_p.to_vec(), Arc::new(c.clone())));
                }
                Cow::Owned(c)
            }
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
        let locked = self.locked_domains.clone();
        sync_system_bindings_to_doc_with_sections(
            &mut next_doc,
            state,
            design_p,
            epistemic_theta,
            |d, s| extract_system_section_grid(d, s, Some(&locked)),
        )?;
        Ok(next_doc)
    }

    /// Extract station-by-station field profiles, 3D cross-section summaries, converged objectives,
    /// and 3D boundary projection summaries for UI & MCP.
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

        let sections = self
            .spec
            .sections
            .iter()
            .enumerate()
            .map(|(sec_idx, sec)| {
                let values: Vec<f64> = (0..nodes)
                    .map(|i| {
                        comp.section_grid
                            .get(sec_idx * nodes + i)
                            .copied()
                            .unwrap_or(0.0)
                    })
                    .collect();
                let entry = values.first().copied().unwrap_or(0.0);
                let exit = values.last().copied().unwrap_or(0.0);
                let min = values.iter().copied().fold(f64::INFINITY, f64::min);
                let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let mean = if values.is_empty() {
                    0.0
                } else {
                    values.iter().sum::<f64>() / (values.len() as f64)
                };
                DeclarativeSectionSummary {
                    name: sec.name.clone(),
                    prop: sec.prop.name().to_string(),
                    mode: sec.mode.name().to_string(),
                    shape: sec.shape.clone(),
                    unit: sec.prop.unit().to_string(),
                    min,
                    max,
                    mean,
                    entry,
                    exit,
                    values,
                }
            })
            .collect();

        let obj_vals = comp.eval_objectives(state, design_p, epistemic_theta);
        let objectives = self
            .spec
            .objectives
            .iter()
            .zip(obj_vals)
            .map(|(obj, value)| DeclarativeObjectiveSummary {
                name: obj.name.clone(),
                symbol: if obj.symbol.is_empty() {
                    obj.name.clone()
                } else {
                    obj.symbol.clone()
                },
                unit: obj.unit.clone(),
                kind: match obj.kind {
                    DeclObjKind::Minimize => "minimize".into(),
                    DeclObjKind::Maximize => "maximize".into(),
                    DeclObjKind::Upper { limit } => format!("<= {limit}"),
                    DeclObjKind::Lower { limit } => format!(">= {limit}"),
                    DeclObjKind::Band { min, max } => format!("[{min} .. {max}]"),
                },
                value,
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
            sections,
            objectives,
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
        let n_sec = self.spec.sections.len();
        let fallback_geom_mm = self
            .spec
            .designs
            .iter()
            .enumerate()
            .find(|(_, d)| d.category == DeclDesignCategory::Geometry)
            .and_then(|(k, _)| design_p.get(k).copied())
            .unwrap_or(1.0);
        let geom_label = if let Some(sec0) = self.spec.sections.first() {
            format!("{} ({})", sec0.name, sec0.prop.name())
        } else {
            self.spec
                .designs
                .iter()
                .find(|d| d.category == DeclDesignCategory::Geometry)
                .map(|d| d.name.clone())
                .unwrap_or_else(|| "Geometry Lever".to_string())
        };
        let st0 = &self.spec.states[0];
        let st1_idx = if n_fields >= 2 { 1 } else { 0 };
        let points = (0..comp.nodes)
            .map(|i| {
                let s_mm = comp.stations[i];
                let geom_mm = if n_sec > 0 {
                    let v = comp.section_grid.get(i).copied().unwrap_or(0.0);
                    match self.spec.sections[0].prop {
                        DeclSectionProp::Area | DeclSectionProp::FillArea => {
                            (4.0 * v.max(0.0) / std::f64::consts::PI).sqrt()
                        }
                        _ => v,
                    }
                } else {
                    fallback_geom_mm
                };
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

    #[test]
    fn sdf_and_tpms_sections_match_closed_form_geometry_and_exact_adjoint_sensitivities() {
        // 1. Hollow Hagen-Poiseuille pipe: `dp_pa = (8 * mu * L * Q) / (pi * r_bore^4)`,
        //    so `d(dp_pa)/d(r_bore) = -4 * dp_pa / r_bore` exactly!
        let pipe_src = "\
fieldsmith 1
name \"Analytical SDF Section Pipe\"
material ss316l
let r_bore = 5
cylinder outer_pipe r=10 h=80 at=(0, 0, 40)
cylinder inner_bore r=r_bore h=84 at=(0, 0, 40) subtract
region bore_wall = inner_bore.side
region hot_ext = outer_pipe.side
region base_face = outer_pipe.bottom
support base_fix base_face fixed=xyz
convection bore_conv bore_wall h=0.002 tinf=25 case=\"Thermal\"
heatflux outer_q hot_ext q=0.08 case=\"Thermal\"

system sdf_pipe title=\"3D SDF Hagen-Poiseuille Pipe\" nodes=21 span=(0, 80) axis=z {
  design r_bore nominal=5 min=3 max=7.5 unit=\"mm\" category=geometry
  state P_kpa init=\"200\" scale=100 unit=\"kPa\"
  state T_f init=\"25\" scale=50 unit=\"C\"
  section A_f prop=area
  section P_wet prop=perimeter
  section D_h prop=dh
  section N_ch prop=count
  section A_wall prop=area mode=solid
  aux q_m3_s expr=\"2e-5\"
  aux mu_pas expr=\"0.001\"
  aux dp_kpa_ds expr=\"(128 * mu_pas * q_m3_s / (pi * D_h^4)) * 1e6\"
  aux h_film expr=\"3.66 * 0.6 / D_h\"
  residual P_kpa eq=\"d_ds_upwind(P_kpa) + dp_kpa_ds\" bc_left=\"P_kpa - 200\"
  residual T_f eq=\"83.64 * d_ds_upwind(T_f) - P_wet * 0.08\" bc_left=\"T_f - 25\"
  objective dp_total_kpa expr=\"entry(P_kpa) - exit(P_kpa)\" reduce=entry kind=minimize unit=\"kPa\"
  objective exit_temp expr=\"T_f\" reduce=exit kind=upper limit=90 unit=\"C\"
  bind bore_conv attr=tinf expr=\"T_f\"
  bind bore_conv attr=h expr=\"h_film * 1e-3\"
}
solve size=6 tref=20
assert \"Poiseuille pressure drop\" metric=\"sys.dp_total_kpa\" target=0.006519 tol=\"1%\" unit=\"kPa\"
assert \"Extracted bore area\" metric=\"section.A_f.mean\" target=78.5398 tol=\"0.25%\" unit=\"mm^2\"
assert \"Extracted annular wall area\" metric=\"section.A_wall.mean\" target=235.6194 tol=\"0.25%\" unit=\"mm^2\"
";
        let doc = fsm::parse(pipe_src).expect("parse pipe .fsm");
        let sys = DeclarativeCoupledSystem::from_document(&doc).expect("compile sdf_pipe");
        let cfg = sys.engine_config();
        let p = sys.nominal_design_vector();
        let th = sys.nominal_epistemic_vector();
        let res =
            MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p, &th).expect("solve sdf_pipe");
        let vis = sys.extract_visual_fields(&res.state, &p, &th);

        let a_exact = std::f64::consts::PI * 25.0;
        let p_exact = 2.0 * std::f64::consts::PI * 5.0;
        let a_wall_exact = std::f64::consts::PI * (100.0 - 25.0);
        assert!(
            ((vis.sections[0].mean - a_exact) / a_exact).abs() < 1e-3,
            "A_f = {} vs exact {}",
            vis.sections[0].mean,
            a_exact
        );
        assert!(
            ((vis.sections[1].mean - p_exact) / p_exact).abs() < 1e-3,
            "P_wet = {} vs exact {}",
            vis.sections[1].mean,
            p_exact
        );
        assert!((vis.sections[2].mean - 10.0).abs() < 2e-2);
        assert_eq!(vis.sections[3].mean, 1.0);
        assert!(
            ((vis.sections[4].mean - a_wall_exact) / a_wall_exact).abs() < 1e-3,
            "A_wall = {} vs exact {}",
            vis.sections[4].mean,
            a_wall_exact
        );

        // Exact Hagen-Poiseuille pressure drop:
        // dp_pa = 8 * 0.001 * 0.080 * 2e-5 / (pi * 0.005^4) = 6.518986 Pa = 0.006518986 kPa
        // Wait: check `dp_kpa_ds` scaling in test formula:
        // D_h in mm => D_h_m = D_h * 1e-3 => D_h_m^4 = D_h^4 * 1e-12.
        // dp/dz [Pa/m] = 128 * mu * Q / (pi * D_h^4 * 1e-12) = (128 * mu * Q / (pi * D_h^4)) * 1e12 Pa/m
        // In kPa/mm: 1 Pa/m = 1e-6 kPa/mm => factor is 1e6! Wait: 128 * 0.001 * 2e-5 / (pi * 10^4) * 1e6 * 80 = 0.006518986 kPa!
        let dp_exact_kpa =
            (128.0 * 0.001 * 2e-5 / (std::f64::consts::PI * 10.0_f64.powi(4))) * 1e6 * 80.0;
        let dp_num = res.objectives[0];
        assert!(
            ((dp_num - dp_exact_kpa) / dp_exact_kpa).abs() < 3e-3,
            "dp_num={dp_num} vs dp_exact={dp_exact_kpa}"
        );
        // Exact analytical geometric sensitivity: d(dp)/d(r_bore) = -4 * dp / r_bore!
        let d_dp_dr_exact = -4.0 * dp_exact_kpa / 5.0;
        let d_dp_dr_adj = res.design_sensitivities[0][0];
        let sens_rel_err = ((d_dp_dr_adj - d_dp_dr_exact) / d_dp_dr_exact).abs();
        assert!(
            sens_rel_err < 5e-3,
            "1D adjoint d(dp)/d(r_bore) = {d_dp_dr_adj:.6e} vs exact -4*dp/r = {d_dp_dr_exact:.6e} (rel_err={sens_rel_err:.4e})"
        );

        // 2. Two-way 3D staggered sensitivity (`3D SDF -> 1D section -> 1D h_film & T_f -> 3D P2 thermo-elastic FEA`):
        // Widening `r_bore` reduces `h_film = 3.66 * k_f / (2 * r_bore)` (`Nu = 3.66` constant)
        // AND increases wetted bore area `2 * pi * r_bore * L` (exact cancellation in `h * A_surf`
        // for total conductance, while wall thickness `R_out - r_bore` thins, reducing conductive
        // temperature drop across the pipe wall so outer peak 3D temperature `T_max` drops: `dT_max/dr_bore < 0`!).
        let sens_3d = crate::sensitivity::compute_sensitivities(
            &doc,
            &["r_bore".to_string()],
            crate::Resolution::Size(6.0),
            crate::now_ms,
        )
        .expect("3D staggered sensitivity through 1D section");
        let ps = &sens_3d.parameters[0];
        assert!(
            ps.d_mass_g < 0.0,
            "widening bore must reduce solid mass, got d_mass_g={}",
            ps.d_mass_g
        );
        let d_tmax = ps.cases[0]
            .d_max_temp
            .expect("thermal sensitivity d_max_temp");
        assert!(
            d_tmax < 0.0,
            "thinning heated cylinder wall must lower peak 3D outer temperature, got d_max_temp={d_tmax}"
        );

        let (_, _, vrep) = crate::contracts::verify_document(
            &doc,
            Some(pipe_src),
            crate::Resolution::Size(6.0),
            crate::now_ms,
        )
        .expect("verify_document on sdf_pipe");
        assert!(
            vrep.all_passed,
            "expected all 3 sdf_pipe contracts to pass: {:?}",
            vrep.outcomes
        );

        // 3. Graded-TPMS `fill_*` cross-section extraction & `let`-driven cell sensitivity:
        let tpms_src = "\
fieldsmith 1
name \"Analytical Gyroid Section Duct\"
material al6061
let cell_a = 5
cylinder duct r=10 h=50 at=(0, 0, 25)
fill gyroid sheet cell=cell_a rho=(0.25, 0.25) wall=0.2 skin=1.5

system tpms_duct nodes=11 span=(0, 50) axis=z {
  design cell_a nominal=5 min=3 max=8 unit=\"mm\" category=geometry
  state P_kpa init=\"300\" scale=100 unit=\"kPa\"
  section A_core prop=fill_area
  section eps_f prop=fill_porosity
  section rho_s prop=fill_rho
  section K_mm2 prop=fill_perm
  section beta_mm prop=fill_beta
  section a_wet prop=fill_awet
  aux u_d_ms expr=\"0.05 / (997 * A_core * 1e-6)\"
  aux dp_kpa_ds expr=\"((8.9e-4 / (K_mm2 * 1e-6)) * u_d_ms + (beta_mm * 1e3) * 997 * u_d_ms^2) * 1e-6\"
  residual P_kpa eq=\"d_ds_upwind(P_kpa) + dp_kpa_ds\" bc_left=\"P_kpa - 300\"
  objective dp_kpa expr=\"entry(P_kpa) - exit(P_kpa)\" reduce=entry kind=minimize unit=\"kPa\"
}
";
        let tdoc = fsm::parse(tpms_src).expect("parse tpms_duct");
        let tsys = DeclarativeCoupledSystem::from_document(&tdoc).expect("compile tpms_duct");
        let tp = tsys.nominal_design_vector();
        let tth = tsys.nominal_epistemic_vector();
        let tres =
            MonolithicSystemEngine::solve_and_analyze(&tsys, &tsys.engine_config(), &tp, &tth)
                .expect("solve tpms_duct");
        let tvis = tsys.extract_visual_fields(&tres.state, &tp, &tth);
        let a_core_exact = std::f64::consts::PI * 8.5_f64.powi(2);
        assert!(
            ((tvis.sections[0].mean - a_core_exact) / a_core_exact).abs() < 1.5e-3,
            "A_core={} vs exact {}",
            tvis.sections[0].mean,
            a_core_exact
        );
        assert!((tvis.sections[1].mean - 0.75).abs() < 1e-6);
        assert!((tvis.sections[2].mean - 0.25).abs() < 1e-6);
        // Enlarging Gyroid unit cell `cell_a` increases permeability `K ~ cell_a^2` and reduces
        // Forchheimer `beta ~ 1/cell_a`, so pressure drop MUST decrease (`d(dp_kpa)/d(cell_a) < 0`)!
        assert!(
            tres.design_sensitivities[0][0] < 0.0,
            "expected d(dp_kpa)/d(cell_a) < 0, got {}",
            tres.design_sensitivities[0][0]
        );
    }
}
