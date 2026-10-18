//! Graded lattice fill (GOAL D): a TPMS lattice inside the part's body, its density driven by
//! the solved stress of a load case (or of all cases), the lattice stiffened by coupon-measured
//! homogenisation laws, every case re-solved on the same mesh, and the final mass in grams.
//!
//! The scheme is single-pass: a solid uniform-modulus solve provides the driving stress, the
//! graded parameter field is built from it, and every case re-solves with the graded per-element
//! moduli. Densities are not re-iterated from the graded stress; V7 measures what one pass
//! achieves against the resolved lattice model.
//!
//! # Homogenised stiffness laws
//!
//! Each family/mode pair gets a law `E* = c·ρ^n`, least-squares fitted to resolved
//! finite-element compression coupons produced by `fieldsmith-verify/examples/tpms-coupon.rs`
//! (the RESULT lines of the sweep logs). The fits are in MPa for the coupon's material, whose
//! solid modulus is 1000 MPa; `Law::at` returns the same law relative to the solid modulus.
//! Conventions:
//!
//! * Sheet laws: coupons of 3 cells with a 1-cell interior strain band and mesh-to-wall factor
//!   1; the fits sit within 0.9–8.4 % of their three coupon points (worst: diamond's ρ = 0.3
//!   point, which was measured on a different tie realisation).
//! * Network laws: coupons of 4 cells, band 1, mesh-to-wall factor 0.5; the fits sit within 1 %.
//!
//! Running the other convention moves measured E* by: gyroid/network −7…−14.5 %,
//! diamond/network −4.7…−9.5 %, primitive/network +34…+51 % (primitive's interior strain is not
//! affine over a one-cell band, so its network law carries the wider uncertainty). Sheet
//! cross-checks at a finer mesh (factor 0.5) drift ≤ 6.9 %.
//!
//! All laws were measured on coupon densities in `[RHO_LO, RHO_HI]`; `Law::at` (and
//! `law_stiffness`) evaluates at the nearest bound outside the band. The engine does not use
//! that clamp directly: each element's modulus comes from its sampled material fraction
//! through `element_stiffness`, which blends between Voigt mixing (element ≲ cell, the mesh
//! resolves real wall/skin geometry) and the coupon law on a continuously extended range
//! (`law_extended`: linear to the band's lower bound, measured law on the band, linear blend
//! to the solid modulus at φ = 1). V7 measured the mapping's sub-cell end at −2.1 % and the
//! defect it replaces (per-centroid law, capped at the 0.40 stiffness for all wall elements)
//! at +21…+38 % on the same beam; between h/cell = 1 and 1.5 the blend is an interpolation,
//! not a measurement. `rho_elem` carries each element's material fraction.

use fieldsmith_doc::{Family, Fill, FillMode, Op};
use fieldsmith_field::expr::{self, Expr};
use fieldsmith_field::lattice::{calibration, FillExpr, Grid3};
use fieldsmith_field::volume;
use fieldsmith_field::{Aabb, V3};
use fieldsmith_solve::P2Mesh;

use crate::Facet;
use crate::{
    case_force_report, case_names, fixed_mask, material_of, run_case, run_thermal_case, Analysis,
    CaseResult, Clock, Resolution, SetupError,
};

/// Fit band of the stiffness laws, densities in `[RHO_LO, RHO_HI]`.
pub const RHO_LO: f64 = 0.18;
pub const RHO_HI: f64 = 0.40;

/// Stiffness floor for fully void elements, relative to the solid modulus. Enclosed void stays
/// solvable; the envelope still reads its stress, but the void's strain energy is noise.
const VOID_SCALE: f64 = 1e-6;

/// Conduction floor for void elements, relative to the pinned solid conductivity: analog of
/// [`VOID_SCALE`], keeping an enclosed void's Laplace rows solvable. Still-air conduction
/// (~2.6e-8 W/(mm·K)) is not modelled — a void reads as effectively insulated.
const K_VOID_SCALE: f64 = 1e-6;

/// Relative conductivity of one graded element ([`fieldsmith_solve::solve_conduction`]'s `k`): solid
/// elements conduct at the pinned unit, everything else at its material fraction φ floored
/// at [`K_VOID_SCALE`]. Linear-in-φ is the Voigt-style map — near-exact where the mesh
/// resolves the walls (sub-cell), over-reading k where a coarse mesh homogenises tortuous
/// walls (the same caveat the stiffness law documents for its sub-cell end). No conduction
/// law is measured, so the map pretends no fitted constant.
pub fn conductivity_of(class: u8, phi: f64) -> f64 {
    if class == CLASS_SOLID {
        1.0
    } else {
        phi.max(K_VOID_SCALE)
    }
}

/// Jittered samples per element when measuring its material fraction (φ). 32 bounds the
/// per-element sampling noise to ~±0.09 at φ = 0.3, which averages out over the thousands of
/// elements a pocket spans; each element's sample set is seeded from its own index, so the
/// field is deterministic regardless of iteration order.
const PHI_SAMPLES: u32 = 32;

/// Material-fraction thresholds separating the reported classes: an element sampled fully
/// material is solid, fully void is void, anything between is a lattice mixture.
const PHI_SOLID: f64 = 0.98;
const PHI_VOID: f64 = 0.02;

/// Per-element class of the graded part.
pub const CLASS_SOLID: u8 = 0;
pub const CLASS_LATTICE: u8 = 1;
pub const CLASS_VOID: u8 = 2;

/// Largest node count a grade grid may reach (memory guard; three f32 grids plus two copies
/// during the blur). A hit here means the cell size is a small fraction of the part.
const MAX_GRID_NODES: usize = 16_000_000;

/// Solid modulus of the coupon material the laws were fitted against (MPa:
/// `tpms-coupon.rs`'s `young=1000`). Dividing the fitted `c` by it turns the law into the
/// relative stiffness the engine uses.
const COUPON_E: f64 = 1000.0;

/// One homogenised stiffness law: `E* / E_solid = c·ρ^n` on the measured density band, with
/// `c` as measured in MPa on the 1000 MPa coupon.
#[derive(Clone, Copy, Debug)]
struct Law {
    c: f64,
    n: f64,
}

impl Law {
    /// Relative stiffness at a lattice density (dimensionless, ≤ 1 on the measured band).
    fn at(&self, rho: f64) -> f64 {
        (self.c / COUPON_E) * rho.clamp(RHO_LO, RHO_HI).powf(self.n)
    }
}

/// Coupon-measured law for a family/mode pair. Provenance and conventions: module header.
fn law(family: Family, mode: FillMode) -> Law {
    match (family, mode) {
        (Family::Gyroid, FillMode::Sheet) => Law { c: 616.9, n: 1.426 },
        (Family::Primitive, FillMode::Sheet) => Law { c: 874.3, n: 2.028 },
        (Family::Diamond, FillMode::Sheet) => Law { c: 585.8, n: 1.208 },
        (Family::Gyroid, FillMode::Network) => Law { c: 858.6, n: 2.133 },
        (Family::Primitive, FillMode::Network) => Law { c: 849.9, n: 1.406 },
        (Family::Diamond, FillMode::Network) => Law { c: 706.5, n: 2.089 },
    }
}

/// The law's relative stiffness at a lattice density — the reference V6 checks the resolved
/// coupon measurements against (and the value the engine assigns lattice elements, modulo the
/// material's own modulus). Exposed for the verifier; the engine's per-element use is via
/// `graded`.
pub fn law_stiffness(family: Family, mode: FillMode, rho: f64) -> f64 {
    law(family, mode).at(rho)
}

/// The measured law extended continuously over the whole density range: linear from 0 at φ = 0
/// to the law's value at `RHO_LO`, the measured law on the band, then linear from the band-top
/// value up to 1 at φ = 1 — so an element that is (nearly) solid material recovers (nearly)
/// the solid modulus, which the bare band clamp would cap at the 0.40 value.
fn law_extended(family: Family, mode: FillMode, phi: f64) -> f64 {
    if phi <= 0.0 {
        return 0.0;
    }
    let l = law(family, mode);
    if phi < RHO_LO {
        return l.at(RHO_LO) * phi / RHO_LO;
    }
    if phi <= RHO_HI {
        return l.at(phi);
    }
    let s_hi = l.at(RHO_HI);
    s_hi + (phi - RHO_HI) / (1.0 - RHO_HI) * (1.0 - s_hi)
}

/// Relative stiffness of a filled element from its material fraction φ and the mesh-to-cell
/// ratio h/cell. Two regimes (both measured on the V7 beam, see the module header): at element
/// sizes below the cell the element resolves actual wall/skin geometry and Voigt mixing (φ) is
/// the bending-consistent answer; at one-and-a-half cells and beyond the element spans whole
/// cells and the coupon-measured law applies. Between h/cell = 1 and 1.5 the two are blended
/// linearly in w — an interpolation, not a measurement.
fn element_stiffness(family: Family, mode: FillMode, phi: f64, h_over_cell: f64) -> f64 {
    let w = ((h_over_cell - 1.0) / 0.5).clamp(0.0, 1.0);
    (1.0 - w) * phi + w * law_extended(family, mode, phi)
}

/// Material fraction of a P2 tetrahedron, by jittered uniform sampling of the fill field.
fn phi_of(mesh: &P2Mesh, e: usize, filled: &Expr, samples: u32, rng: &mut Lcg) -> f64 {
    let tet = mesh.tets[e];
    let v = [tet[0], tet[1], tet[2], tet[3]].map(|n| mesh.nodes[n as usize]);
    let mut inside = 0u32;
    let mut lam = [0.0f64; 4];
    for _ in 0..samples {
        // Dirichlet(1,1,1,1): exponentials of uniforms, normalised.
        let mut s = 0.0;
        for l in &mut lam {
            *l = -(rng.next_f64() + 1e-12).ln();
            s += *l;
        }
        let mut pt = [0.0f64; 3];
        for c in 0..3 {
            pt[c] = (lam[0] * v[0][c] + lam[1] * v[1][c] + lam[2] * v[2][c] + lam[3] * v[3][c]) / s;
        }
        if expr::eval(filled, pt).v < 0.0 {
            inside += 1;
        }
    }
    inside as f64 / samples as f64
}

/// Deterministic LCG (the same constants as the verifier's probes) for jittered φ sampling.
struct Lcg(u64);
impl Lcg {
    fn next_f64(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / (1u64 << 53) as f64
    }
}

/// The graded-fill result: everything the solid analysis shares (mesh, case order, loads) plus
/// the graded re-solve and the fill itself.
#[derive(Clone, Debug)]
pub struct Graded {
    /// The fill spec as authored in the document.
    pub spec: Fill,
    /// The filled part's field, `param` carrying the graded parameter grid (mm).
    pub fill: FillExpr,
    /// Per-element modulus, same element order as the analysis mesh.
    pub young: Vec<f64>,
    /// Per-element class from the sampled material fraction φ (see `rho_elem`): `CLASS_SOLID`
    /// (φ ≥ 0.98, e.g. skin and protected zones), `CLASS_VOID` (φ ≤ 0.02, pockets),
    /// `CLASS_LATTICE` (partially filled).
    pub class: Vec<u8>,
    /// Per-element material fraction of the fill (φ), the sampled quantity the modulus is
    /// built from: 1.0 fully solid, 0.0 fully void.
    pub rho_elem: Vec<f64>,
    /// (min, max) of the grading design density over the lattice core — the achieved range
    /// after wall-thickening clamps, i.e. the range the law would be evaluated over at
    /// homogenised mesh sizes. Kept so callers can show whether the design sits inside the
    /// laws' measured band.
    pub rho_design: [f64; 2],
    /// Graded re-solve, one per solid case, same order.
    pub cases: Vec<CaseResult>,
    /// Thermal cases re-solved through the graded core, in `doc.thermal_cases()` order: the
    /// conduction solve carries the fill's per-element conductivity map, the expansion solve
    /// the graded moduli. Same case type as the solid path's thermal results.
    pub thermal_cases: Vec<CaseResult>,
    /// Envelope over the graded cases: nodal and per-element von Mises.
    pub envelope: Vec<f64>,
    pub envelope_elem: Vec<f64>,
    /// Driving stress at which the grade reaches ρmax (90th percentile over lattice-region
    /// elements; above it the density clamps at ρmax).
    pub sigma_ref: f64,
    /// Final mass (g), integrated from the field with the material's density.
    pub mass_g: f64,
    /// Mass of the same part without the fill (g), integrated the same way.
    pub solid_mass_g: f64,
    pub fill_ms: f64,
    pub solve_ms: f64,
}

/// Cheap numeric spec checks shared by [`graded_of`] and [`preview_fill`]; the case
/// resolution and the stress machinery stay graded-only.
fn check_spec(spec: &Fill) -> Result<(), SetupError> {
    if spec.cell <= 0.0 {
        return Err(SetupError("fill cell size must be positive".into()));
    }
    if spec.skin < 0.0 || spec.margin < 0.0 || spec.min_wall < 0.0 {
        return Err(SetupError(
            "fill skin, margin and min wall must be non-negative".into(),
        ));
    }
    if spec.curve <= 0.0 {
        return Err(SetupError("fill curve must be positive".into()));
    }
    if spec.rho_min <= 0.0 || spec.rho_min > spec.rho_max || spec.rho_max > 1.0 {
        return Err(SetupError(
            "fill densities must satisfy 0 < ρmin ≤ ρmax ≤ 1".into(),
        ));
    }
    Ok(())
}

/// Union of the fill's protected zones: each protected shape as placed (patterns
/// included), offset outward by the margin. `Empty` when the fill protects nothing.
fn keep_zones(doc: &fieldsmith_doc::Document, spec: &Fill) -> Result<Expr, SetupError> {
    let mut keep = Expr::Empty;
    for name in &spec.protect {
        let shape = expr::placed_shape(doc, name)
            .ok_or_else(|| SetupError(format!("fill protects missing shape '{name}'")))?;
        let zone = Expr::Offset {
            d: spec.margin,
            e: Box::new(shape),
        };
        keep = match keep {
            Expr::Empty => zone,
            k => Expr::Combine {
                op: Op::Add,
                blend: 0.0,
                a: Box::new(k),
                b: Box::new(zone),
            },
        };
    }
    Ok(keep)
}

/// Pre-analysis fill look for the viewport: the fill the document authors, on the grade grid
/// the analysed build would use (same bounds, spacing and padding as [`graded_of`]) but with
/// the density uniformly at the spec's ρmax — a clearly visible lattice, the deliberate
/// stand-in before the graded parameter field arrives. A uniform grid keeps the viewport's
/// preview→graded swap a texture upload instead of a recompile when the dimensions match.
/// Runs only the cheap spec checks; no load case, no stress probe, no re-solve.
pub fn preview_fill(doc: &fieldsmith_doc::Document) -> Result<FillExpr, SetupError> {
    let spec = doc
        .fill
        .clone()
        .ok_or_else(|| SetupError("this document has no fill".into()))?;
    check_spec(&spec)?;
    let body = expr::compile_body(doc);
    let b = expr::bounds(&body);
    if b.is_empty() {
        return Err(SetupError("the model is empty".into()));
    }
    let keep = keep_zones(doc, &spec)?;
    let cal = calibration(spec.family, spec.mode);
    let mut p0 = cal.param_for(spec.rho_max, spec.cell);
    if spec.mode == FillMode::Sheet {
        p0 = p0.max(spec.min_wall);
    }
    let spacing = (spec.cell / 2.0).max(1e-3);
    let mut param = grid_over(&b, spacing, spec.cell)?;
    if param.dims.iter().any(|&d| d < 2) {
        param = Grid3::constant(p0 as f32);
    } else {
        param.data.iter_mut().for_each(|v| *v = p0 as f32);
    }
    Ok(FillExpr {
        body,
        skin: spec.skin,
        family: spec.family,
        mode: spec.mode,
        cell: spec.cell,
        param,
        keep,
    })
}

/// Solid analysis, then grading and a graded re-solve of every case. One call, both solves.
pub fn graded(
    doc: &fieldsmith_doc::Document,
    res: Resolution,
    clock: Clock,
) -> Result<(Analysis, Graded), SetupError> {
    let base = crate::analyse(doc, res, clock)?;
    let g = graded_of(doc, &base, clock)?;
    Ok((base, g))
}

/// Grade the fill from an existing solid analysis and re-solve. Mesh, supports and loads are the
/// analysis's; only the per-element moduli change.
pub fn graded_of(
    doc: &fieldsmith_doc::Document,
    base: &Analysis,
    clock: Clock,
) -> Result<Graded, SetupError> {
    let t0 = clock();
    let spec = doc
        .fill
        .clone()
        .ok_or_else(|| SetupError("this document has no fill".into()))?;
    check_spec(&spec)?;
    let drive: &Vec<f64> = match &spec.case {
        Some(name) => {
            let c = base.cases.iter().find(|c| c.name == *name).ok_or_else(|| {
                SetupError(format!("fill case '{name}': no load case with that name"))
            })?;
            &c.vm_elem
        }
        None => &base.envelope_elem,
    };
    let cal = calibration(spec.family, spec.mode);
    let mat = material_of(doc)?;
    let body = expr::compile_body(doc);
    let b = expr::bounds(&body);
    if b.is_empty() {
        return Err(SetupError("the model is empty".into()));
    }

    // Protected zones: each protected shape as placed (patterns included), offset outward by
    // the margin, unioned with a plain min so the material around it is kept.
    let keep = keep_zones(doc, &spec)?;

    // A parameter-free probe, purely to read the solid/lattice split (`in_core` never looks at
    // `param`); the real fill expression is built once the grades exist.
    let probe = FillExpr {
        body: body.clone(),
        skin: spec.skin,
        family: spec.family,
        mode: spec.mode,
        cell: spec.cell,
        param: Grid3::constant(0.0),
        keep: keep.clone(),
    };

    // The grade grid: half a cell per node, padded a cell beyond the part so surface-adjacent
    // cells interpolate instead of clamping.
    let spacing = (spec.cell / 2.0).max(1e-3);
    let pad = spec.cell;
    let mut core = grid_over(&b, spacing, pad)?;
    let [nx, ny, nz] = core.dims;
    let (ox, oy, oz) = (core.origin[0], core.origin[1], core.origin[2]);
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                let p = [
                    ox + i as f64 * spacing,
                    oy + j as f64 * spacing,
                    oz + k as f64 * spacing,
                ];
                core.data[(k * ny + j) * nx + i] = if probe.in_core(p) { 1.0 } else { 0.0 };
            }
        }
    }
    let n_nodes = core.data.len();

    // Node density: fallback ρmin, filled from the volume-weighted scatter of the driving
    // stress, normalised at the 90th percentile over lattice-region elements (so the
    // normalisation does not depend on how fine the skin band meshed).
    let mut core_elems: Vec<f64> = Vec::new();
    let mut wsum = vec![0.0f32; n_nodes]; // Σ vol·σ per node
    let mut vsum = vec![0.0f32; n_nodes]; // Σ vol per node
    for (e, sigma_e) in drive.iter().enumerate() {
        let c = centroid(&base.mesh, e);
        if probe.in_core(c) {
            core_elems.push(*sigma_e);
        }
        if let Some(idx) = node_near(&core, c) {
            let v = base.mesh.tet_volume(e) as f32;
            wsum[idx] += v * *sigma_e as f32;
            vsum[idx] += v;
        }
    }
    if core_elems.is_empty() {
        return Err(SetupError(
            "the lattice region is empty: the part is thinner than 2·skin, or every cell is protected"
                .into(),
        ));
    }
    let sigma_ref = quantile(&mut core_elems, 0.9);

    let mut rho = Grid3 {
        origin: core.origin,
        spacing: core.spacing,
        dims: core.dims,
        data: vec![spec.rho_min as f32; n_nodes],
    };
    for i in 0..n_nodes {
        if vsum[i] > 1e-9 {
            let sigma = (wsum[i] / vsum[i]) as f64;
            let t = if sigma_ref > 0.0 {
                (sigma / sigma_ref).clamp(0.0, 1.0)
            } else {
                0.0
            };
            rho.data[i] =
                (spec.rho_min + (spec.rho_max - spec.rho_min) * t.powf(spec.curve)) as f32;
        }
    }
    // Smooth the grades over about a cell (one masked box blur, in-core samples weighted only)
    // so the parameter field varies smoothly rather than per mesh element.
    let r = ((spec.cell / spacing) / 2.0).round().max(1.0) as usize;
    let rho = masked_blur(&rho, &core, r);

    // Parameter grid (mm). Sheet walls are raised to the min printable wall; network struts are
    // not — the ρ↔param relation is family-dependent and the strut thinnest at ρmin is already
    // printable-ish, so network min-wall is disclosed rather than enforced.
    let mut param = grid_over(&b, spacing, pad)?;
    let mut rho_design = [f64::MAX, 0.0f64];
    for (i, &vs) in vsum.iter().enumerate() {
        let mut p = cal.param_for(rho.data[i] as f64, spec.cell);
        if spec.mode == FillMode::Sheet {
            p = p.max(spec.min_wall);
        }
        param.data[i] = p as f32;
        if vs > 1e-9 {
            let achieved = cal.rho_for(p, spec.cell);
            rho_design[0] = rho_design[0].min(achieved);
            rho_design[1] = rho_design[1].max(achieved);
        }
    }

    // Element moduli from each element's sampled material fraction of the actual fill (see
    // `element_stiffness` for the mapping); classes follow from the same φ. The fill expression
    // is the classification's ground truth, so a mesh finer than the walls picks the walls up
    // at full modulus and a mesh coarser than the cells homogenises them through the law.
    let fill = FillExpr {
        body: body.clone(),
        skin: spec.skin,
        family: spec.family,
        mode: spec.mode,
        cell: spec.cell,
        param,
        keep: keep.clone(),
    };
    let filled = Expr::Filled(Box::new(fill.clone()));
    let h_over_cell = base.provenance.element_size / spec.cell;
    let mut young = vec![0.0f64; base.mesh.tets.len()];
    let mut class = vec![0u8; base.mesh.tets.len()];
    let mut rho_elem = vec![0.0f64; base.mesh.tets.len()];
    for (e, y) in young.iter_mut().enumerate() {
        let mut rng = Lcg(0x9E37_79B9_7F4A_7C15 ^ (e as u64).wrapping_mul(0xA076_1D64_78BD_542F));
        let phi = phi_of(&base.mesh, e, &filled, PHI_SAMPLES, &mut rng);
        rho_elem[e] = phi;
        class[e] = if phi >= PHI_SOLID {
            CLASS_SOLID
        } else if phi <= PHI_VOID {
            CLASS_VOID
        } else {
            CLASS_LATTICE
        };
        *y = mat.e * VOID_SCALE.max(element_stiffness(spec.family, spec.mode, phi, h_over_cell));
    }

    // Mass from the field with the material's density (g/cm³ → g/mm³: ÷1000). The fill never
    // exceeds the body, so both integrals cover the same volume; the difference is the lattice.
    let t_fill = clock();
    let mass_h = (spec.cell / 4.0).min(volume::default_spacing(&b));
    let mass_g = volume::integrate(&Expr::Filled(Box::new(fill.clone())), mass_h, None)
        * mat.density
        / 1000.0;
    let solid_mass_g = volume::integrate(&body, mass_h, None) * mat.density / 1000.0;
    let t_solved = clock();

    // Re-solve every case with the graded moduli, on the same mesh and the same loads. The mesh
    // is never trimmed: voids are carried by their stiffness floor.
    let facets: Vec<Facet> = base.surface.iter().map(|&nodes| Facet { nodes }).collect();
    let h = base.provenance.element_size;
    let fixed = fixed_mask(doc, &base.mesh, &facets, h)?;
    let mut cases = Vec::new();
    for name in case_names(doc) {
        let (force, reports) = case_force_report(doc, &base.mesh, &facets, &name, h)?;
        let mut case = run_case(&base.mesh, &young, mat.nu, &fixed, force, name)?;
        case.load_reports = reports;
        cases.push(case);
    }

    // Thermal cases re-solved through the graded core: conduction runs on the fill's
    // per-element conductivity map (solid pinned at 1, lattice/void scaled by φ — see
    // `conductivity_of`), the expansion solve on the graded moduli. The boundary statements
    // keep their unit-solid reading, same contract as the static path's thermal block.
    let thermal_names = doc.thermal_cases();
    let mut thermal_cases = Vec::new();
    if !thermal_names.is_empty() {
        let alpha = mat.alpha.ok_or_else(|| {
            SetupError(format!(
                "The thermal flow needs a thermal-expansion coefficient and material '{}' \
                 states none — pick a material whose sheet states a CTE, or give a custom \
                 material an a= value",
                doc.material
            ))
        })?;
        // Rebuilt from the stored classes and fractions (same call the wasm test uses), so
        // the map reproduced from the result matches the one this block solved with exactly.
        let k_elem: Vec<f64> = class
            .iter()
            .zip(&rho_elem)
            .map(|(&c, &phi)| conductivity_of(c, phi))
            .collect();
        for name in thermal_names {
            let case = run_thermal_case(
                doc, &base.mesh, &young, mat.nu, alpha, &fixed, &facets, name, h, &k_elem,
            )?;
            thermal_cases.push(case);
        }
    }
    let nn = base.mesh.nodes.len();
    let envelope = (0..nn)
        .map(|i| cases.iter().map(|c| c.von_mises[i]).fold(0.0, f64::max))
        .collect();
    let envelope_elem = (0..base.mesh.tets.len())
        .map(|e| cases.iter().map(|c| c.vm_elem[e]).fold(0.0, f64::max))
        .collect();

    Ok(Graded {
        spec,
        fill,
        young,
        class,
        rho_elem,
        rho_design,
        cases,
        thermal_cases,
        envelope,
        envelope_elem,
        sigma_ref,
        mass_g,
        solid_mass_g,
        fill_ms: t_fill - t0,
        solve_ms: clock() - t_solved,
    })
}

/// A grid over the part's bounding box, padded by `cell` on every axis, filled with zeros.
fn grid_over(b: &Aabb, spacing: f64, pad: f64) -> Result<Grid3, SetupError> {
    let mut origin = [0.0f64; 3];
    let mut dims = [1usize; 3];
    for a in 0..3 {
        origin[a] = b.lo[a] - pad;
        dims[a] = (((b.hi[a] + pad) - (b.lo[a] - pad)) / spacing).ceil() as usize + 1;
    }
    let n = dims
        .iter()
        .try_fold(1usize, |acc: usize, &d| acc.checked_mul(d))
        .filter(|&n| n <= MAX_GRID_NODES)
        .ok_or_else(|| {
            SetupError(format!(
                "the grade grid would be too large ({}×{}×{} nodes); increase the fill cell size",
                dims[0], dims[1], dims[2]
            ))
        })?;
    Ok(Grid3 {
        origin,
        spacing,
        dims,
        data: vec![0.0f32; n],
    })
}

/// Centroid of a tet's four CORNER nodes — tets carry 10 P2 nodes and iterating the array
/// would pull the average towards the midside nodes.
fn centroid(mesh: &fieldsmith_solve::P2Mesh, e: usize) -> V3 {
    let corners = [
        mesh.tets[e][0],
        mesh.tets[e][1],
        mesh.tets[e][2],
        mesh.tets[e][3],
    ];
    let mut c = [0.0f64; 3];
    for n in corners.iter() {
        let p = mesh.nodes[*n as usize];
        for a in 0..3 {
            c[a] += p[a];
        }
    }
    c.map(|x| x / 4.0)
}

/// Index of the grid node nearest a point, or None past the grid's edge (the padding zones
/// should not receive scattered stress).
fn node_near(g: &Grid3, p: V3) -> Option<usize> {
    let [nx, ny, nz] = g.dims;
    let mut idx = [0usize; 3];
    for a in 0..3 {
        let n = [nx, ny, nz][a];
        let u = (p[a] - g.origin[a]) / g.spacing;
        if u < -0.5 || u > n as f64 - 0.5 {
            return None;
        }
        idx[a] = u.round().clamp(0.0, (n - 1) as f64) as usize;
    }
    Some((idx[2] * ny + idx[1]) * nx + idx[0])
}

/// One box-blur pass over the grid, weighted by an in/out mask. Masked-out nodes keep their
/// value (the fallback ρmin), so the blur never bleeds into void zones.
fn masked_blur(a: &Grid3, mask: &Grid3, r: usize) -> Grid3 {
    let [nx, ny, nz] = a.dims;
    let mut out = a.clone();
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                let base = (k * ny + j) * nx + i;
                if mask.data[base] <= 0.0 {
                    continue;
                }
                let mut sum = 0.0f32;
                let mut wsum = 0.0f32;
                for dk in 0..(2 * r + 1) {
                    let kk = (k + dk) as isize - r as isize;
                    let kk = kk.clamp(0, nz as isize - 1) as usize;
                    for dj in 0..(2 * r + 1) {
                        let jj = (j + dj) as isize - r as isize;
                        let jj = jj.clamp(0, ny as isize - 1) as usize;
                        for di in 0..(2 * r + 1) {
                            let ii = (i + di) as isize - r as isize;
                            let ii = ii.clamp(0, nx as isize - 1) as usize;
                            let idx = (kk * ny + jj) * nx + ii;
                            let w = mask.data[idx];
                            sum += a.data[idx] * w;
                            wsum += w;
                        }
                    }
                }
                if wsum > 0.0 {
                    out.data[base] = sum / wsum;
                }
            }
        }
    }
    out
}

/// q-quantile by sort (the caller guarantees a non-empty slice).
fn quantile(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    let i = (((v.len() - 1) as f64) * q).round() as usize;
    v[i]
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldsmith_doc::fsm;

    fn parse(src: &str) -> fieldsmith_doc::Document {
        fsm::parse(&format!("fieldsmith 1\n{src}")).expect("valid test document")
    }

    fn now() -> f64 {
        use std::time::Instant;
        thread_local!(static T0: Instant = Instant::now());
        T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
    }

    /// Tension bar with a graded gyroid-sheet fill.
    const BAR: &str = "\
name \"Fill test bar\"
custommaterial steel200 e=200000 nu=0.3 yield=250 density=7.85
material steel200
box bar size=(40, 10, 10) at=(20, 5, 5)
region cap = bar.nx
region tip = bar.px
support clamp cap fixed=xyz
load pull tip force=(1200, 0, 0)
fill gyroid sheet cell=4 skin=1 rho=(0.2, 0.6)
";

    #[test]
    fn graded_bar_smoke() {
        let d = parse(BAR);
        let (base, g) = graded(&d, Resolution::Size(1.5), now).expect("graded analysis");
        assert_eq!(g.cases.len(), base.cases.len());
        let solid = &base.cases[0];
        let graded_case = &g.cases[0];
        assert!(
            graded_case.solve.converged,
            "re-solve did not converge: {:?}",
            graded_case.solve
        );

        // The lattice is softer than the solid bar, so it deflects more.
        assert!(
            graded_case.max_displacement > solid.max_displacement,
            "graded deflection {:.6} vs solid {:.6}",
            graded_case.max_displacement,
            solid.max_displacement
        );

        // Classes: a real lattice with skin and pockets.
        let (mut n_solid, mut n_lat, mut n_void) = (0usize, 0usize, 0usize);
        for &k in &g.class {
            match k {
                CLASS_SOLID => n_solid += 1,
                CLASS_LATTICE => n_lat += 1,
                _ => n_void += 1,
            }
        }
        assert!(
            n_solid > 0 && n_lat > 0 && n_void > 0,
            "solid={n_solid} lattice={n_lat} void={n_void}"
        );

        // Moduli: every element strictly positive and at most the solid modulus; the modulus is
        // exactly the mapped material fraction (self-consistency of `element_stiffness`);
        // fully solid elements recover the solid modulus exactly and fully void ones sit on
        // the floor.
        let mat = material_of(&d).unwrap();
        let h_over_cell = base.provenance.element_size / g.spec.cell;
        for i in 0..g.young.len() {
            let y = g.young[i];
            let phi = g.rho_elem[i];
            assert!((0.0..=1.0).contains(&phi), "rho_elem[{i}] = {phi}");
            assert!(y > 0.0 && y <= mat.e * (1.0 + 1e-9), "young[{i}] = {y}");
            let mapped = mat.e
                * VOID_SCALE.max(element_stiffness(
                    g.spec.family,
                    g.spec.mode,
                    phi,
                    h_over_cell,
                ));
            assert!(
                (y - mapped).abs() < 1e-6 * mat.e,
                "young[{i}] = {y} does not match the φ mapping ({mapped}) at φ = {phi}"
            );
            if phi >= PHI_SOLID {
                assert!((y - mat.e).abs() < 1e-6 * mat.e, "solid young[{i}] = {y}",);
            } else if phi <= PHI_VOID {
                assert!(y <= mat.e * 1e-4, "void young[{i}] = {y}");
            }
        }

        // Classes follow φ.
        for (i, (&k, &r)) in g.class.iter().zip(&g.rho_elem).enumerate() {
            match k {
                CLASS_SOLID => assert!(r >= PHI_SOLID, "rho_elem[{i}] = {r} on solid"),
                CLASS_LATTICE => assert!(r > PHI_VOID && r < PHI_SOLID, "rho_elem[{i}] = {r}"),
                _ => assert!(r <= PHI_VOID, "rho_elem[{i}] = {r} on void"),
            }
        }

        // The design-density range is surfaced through rho_design: this bar (cell 4, no wall=
        // param) clamps every sheet wall to the 0.8 mm default minimum, so the achieved design
        // density sits at ρ ≈ 0.62 — above the law's measured band, which `rho_design` makes
        // visible (the law itself clamps on the band; the mesh here runs sub-cell so moduli
        // come from φ either way).
        assert!(
            g.rho_design[0] > 0.5 && g.rho_design[1] < 0.75,
            "design density range {:?}",
            g.rho_design
        );

        // Masses in grams, lattice below the solid part, and the field-level volume near the
        // solid one (the fill's skin keeps it above the lattice mass).
        assert!(g.solid_mass_g > 0.0, "solid mass {:.6}", g.solid_mass_g);
        assert!(
            g.mass_g > 0.0 && g.mass_g < g.solid_mass_g,
            "masses {:.6} / {:.6}",
            g.mass_g,
            g.solid_mass_g
        );
        // Bar is 40×10×10 = 4000 mm³ of 7.85 g/cm³ steel ≈ 31.4 g; the solid integral should land
        // with a few percent of that (meshing trims nothing at this h).
        assert!(
            (g.solid_mass_g - 31.4).abs() < 1.0,
            "solid mass {:.3} g",
            g.solid_mass_g
        );

        assert!(g.sigma_ref > 0.0, "sigma_ref = {}", g.sigma_ref);
        assert!(g.fill_ms >= 0.0 && g.solve_ms >= 0.0);

        // The graded readout must also carry a real stress field: the envelope over the graded
        // cases matches the single case's.
        for (m, c) in g.envelope.iter().zip(&graded_case.von_mises) {
            assert!((m - c).abs() < 1e-9);
        }
        assert!(graded_case.max_von_mises > 0.0);
    }

    #[test]
    fn fill_case_error() {
        // A fill case that no load case defines is a setup error naming it.
        let src = BAR.replace(
            "fill gyroid sheet cell=4 skin=1 rho=(0.2, 0.6)",
            "fill gyroid sheet cell=4 skin=1 rho=(0.2, 0.6) case=\"ghost\"",
        );
        let d = parse(&src);
        let err = graded(&d, Resolution::Size(1.5), now).expect_err("ghost case must be rejected");
        assert!(err.0.contains("ghost"), "error: {}", err.0);
        assert!(err.0.contains("no load case"), "error: {}", err.0);
    }

    #[test]
    fn no_fill_error() {
        let src = BAR
            .split("\n")
            .filter(|l| !l.starts_with("fill"))
            .collect::<Vec<_>>()
            .join("\n");
        let d = parse(&src);
        let err = graded(&d, Resolution::Size(1.5), now).expect_err("no fill must be rejected");
        assert!(err.0.contains("no fill"), "error: {}", err.0);
    }

    /// The preview's parameter grid is exactly the graded build's grid (same geometry — the
    /// invariant the viewport's texture-only preview→graded swap relies on) and is uniform
    /// at the ρmax parameter. The uniform value itself is not pinned here (it clamps to the
    /// app's default minimum sheet wall, owned by the document layer's defaults).
    #[test]
    fn preview_grid_matches_the_graded_grid_geometry() {
        let d = parse(BAR);
        let fx = preview_fill(&d).expect("preview fill");
        let (_, g) = graded(&d, Resolution::Size(1.5), now).expect("graded analysis");
        assert_eq!(fx.param.dims, g.fill.param.dims);
        assert_eq!(fx.param.origin, g.fill.param.origin);
        assert_eq!(fx.param.spacing, g.fill.param.spacing);
        let v0 = fx.param.data[0];
        assert!(fx.param.data.iter().all(|&v| v == v0), "preview is uniform");
        assert_eq!(fx.family, g.spec.family);
        assert_eq!(fx.mode, g.spec.mode);
        assert_eq!(fx.cell, g.spec.cell);
        assert_eq!(fx.skin, g.spec.skin);
    }

    #[test]
    fn preview_fill_rejects_bad_numbers() {
        let d = parse(&BAR.replace("rho=(0.2, 0.6)", "rho=(0.6, 0.2)"));
        let err = preview_fill(&d).expect_err("inverted densities must be rejected");
        assert!(err.0.contains("ρmin"), "error: {}", err.0);
        let d = parse(&BAR.replace("cell=4", "cell=0"));
        let err = preview_fill(&d).expect_err("zero cell must be rejected");
        assert!(err.0.contains("cell size"), "error: {}", err.0);
    }

    #[test]
    fn preview_needs_a_fill() {
        let src = BAR
            .split("\n")
            .filter(|l| !l.starts_with("fill"))
            .collect::<Vec<_>>()
            .join("\n");
        let d = parse(&src);
        let err = preview_fill(&d).expect_err("no fill must be rejected");
        assert!(err.0.contains("no fill"), "error: {}", err.0);
    }

    #[test]
    fn protected_neighbourhood_is_solid() {
        // A protected cylinder near the bar's tip: elements around it stay solid, and the
        // lattice lives on beyond the margin.
        let src = BAR
            .replace(
                "fill gyroid sheet cell=4 skin=1 rho=(0.2, 0.6)",
                "fill gyroid sheet cell=4 skin=1 rho=(0.2, 0.6) protect=stub margin=3",
            )
            .replace(
                "region tip = bar.px",
                "region tip = bar.px\ncylinder stub r=3 h=2 at=(30, 5, 5)",
            );
        let d = parse(&src);
        let (base, g) = graded(&d, Resolution::Size(1.5), now).expect("graded analysis");
        let mat = material_of(&d).unwrap();
        let n_lat = g.class.iter().filter(|&&k| k == CLASS_LATTICE).count();
        assert!(n_lat > 0, "lattice vanished with margin=3");
        // Within 1 mm of the stub surface (the margin is 3) every element samples fully
        // material, so its class must be solid. Out to 2 mm an element's sample points can
        // reach past the margin edge, so the guarantee it tests there is stiffness, not class.
        let mut near_solid = 0;
        for e in 0..base.mesh.tets.len() {
            let c = centroid(&base.mesh, e);
            let xy = ((c[0] - 30.0).powi(2) + (c[1] - 5.0).powi(2)).sqrt() - 3.0;
            let dz = (c[2] - 5.0).abs() - 1.0;
            let dist = (xy.max(0.0).powi(2) + dz.max(0.0).powi(2)).sqrt();
            if dist <= 1.0 {
                assert_eq!(
                    g.class[e], CLASS_SOLID,
                    "non-solid class {dist:.2} mm inside the protected zone"
                );
                near_solid += 1;
            } else if dist <= 2.0 {
                assert!(
                    g.young[e] >= 0.8 * mat.e,
                    "young = {:.3}·E only, {dist:.2} mm inside the margin",
                    g.young[e] / mat.e
                );
                near_solid += 1;
            }
        }
        assert!(near_solid > 0, "no elements near the stub");
    }

    /// Tension bar (the fill fixture) with two Dirichlet temperature faces driving a
    /// steady thermal case: cap at 100 °C, tip at 0 °C. `a=` is required — the graded
    /// thermal block runs the same analyse-time CTE guard the static path does.
    const THERMAL_BAR: &str = "\
name \"Fill thermal bar\"
custommaterial steel200 e=200000 nu=0.3 yield=250 density=7.85 a=1.2e-5
material steel200
box bar size=(40, 10, 10) at=(20, 5, 5)
region cap = bar.nx
region tip = bar.px
support clamp cap fixed=xyz
load pull tip force=(1200, 0, 0)
temperature hot cap temp=100 case=\"Thermal 1\"
temperature cold tip temp=0
fill gyroid sheet cell=4 skin=1 rho=(0.2, 0.6)
";

    /// Corner-affine ∇T recovery on a P2 element — same helper as the solve crate's
    /// thermal tests, where it is exact for the piecewise-affine profiles these cases
    /// carry.
    fn affine_gradient(mesh: &P2Mesh, e: usize, t: &[f64]) -> [f64; 3] {
        let tt = &mesh.tets[e];
        let x = [0usize, 1, 2, 3].map(|i| mesh.nodes[tt[i] as usize]);
        let tv = [t[tt[0] as usize], t[tt[1] as usize], t[tt[2] as usize], t[tt[3] as usize]];
        let rhs = [tv[1] - tv[0], tv[2] - tv[0], tv[3] - tv[0]];
        let rows = [0usize, 1, 2].map(|r| {
            [
                x[r + 1][0] - x[0][0],
                x[r + 1][1] - x[0][1],
                x[r + 1][2] - x[0][2],
            ]
        });
        let det = |m: &[[f64; 3]; 3]| {
            m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
                - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
                + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
        };
        let denom = det(&rows);
        let mut g = [0.0f64; 3];
        for (j, gj) in g.iter_mut().enumerate() {
            let mj = [
                [if j == 0 { rhs[0] } else { rows[0][0] }, if j == 1 { rhs[0] } else { rows[0][1] }, if j == 2 { rhs[0] } else { rows[0][2] }],
                [if j == 0 { rhs[1] } else { rows[1][0] }, if j == 1 { rhs[1] } else { rows[1][1] }, if j == 2 { rhs[1] } else { rows[1][2] }],
                [if j == 0 { rhs[2] } else { rows[2][0] }, if j == 1 { rhs[2] } else { rows[2][1] }, if j == 2 { rhs[2] } else { rows[2][2] }],
            ];
            *gj = det(&mj) / denom;
        }
        g
    }

    /// Dissipation ∫ k|∇T|² dV term by term via the corner-affine recovery.
    fn dissipation(mesh: &P2Mesh, t: &[f64], k: &[f64]) -> f64 {
        (0..mesh.tets.len())
            .map(|e| {
                let g = affine_gradient(mesh, e, t);
                k[e] * (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]) * mesh.tet_volume(e).abs()
            })
            .sum()
    }

    #[test]
    fn graded_thermal_flow_goes_through_the_core() {
        let d = parse(THERMAL_BAR);
        let (base, g) = graded(&d, Resolution::Size(1.5), now).expect("graded analysis");

        // Wiring: the graded result carries one result per document thermal case, in
        // document order, each with the mesh's temperature field.
        let names = d.thermal_cases();
        assert_eq!(names, ["Thermal 1"]);
        assert_eq!(g.thermal_cases.len(), names.len());
        let solid_t = base
            .cases
            .iter()
            .find(|c| c.name == "Thermal 1")
            .expect("solid thermal case")
            .temperature
            .as_ref()
            .expect("solid temperature");
        let graded_t = g.thermal_cases[0].temperature.as_ref().expect("graded temperature");
        assert!(
            g.thermal_cases[0].solve.converged
                && g.thermal_cases[0].conduction.as_ref().unwrap().converged,
            "graded thermal case did not converge"
        );
        assert_eq!(graded_t.len(), base.mesh.nodes.len(), "temperature node count");

        // The solid case's field is the affine 100 − 2.5·x (face spacing 40), so its
        // dissipation is the analytic ∫k|∇T|²dV = 1·2.5²·4000 = 25000; check the FE field
        // reproduces it before trusting the graded comparison.
        let nn = base.mesh.nodes.len();
        let solid = dissipation(&base.mesh, solid_t, &vec![1.0; nn]);
        let q = 2.5f64;
        let want = q * q * 4000.0;
        assert!(
            (solid - want).abs() < 5e-6 * want,
            "solid dissipation {solid} vs analytic {want}"
        );

        // The graded field's dissipation with the fill's per-element map (rebuilt from the
        // stored classes and fractions, the same source the solve used) is strictly smaller
        // — k_rel = φ ≤ 1 everywhere and the quadratic-form bound says the graded solve's
        // minimiser over the same trial space dissipates no more than the solid field's
        // restriction to it.
        let k_elem: Vec<f64> = g
            .class
            .iter()
            .zip(&g.rho_elem)
            .map(|(&c, &phi)| conductivity_of(c, phi))
            .collect();
        assert!(k_elem.iter().all(|k| *k > 0.0 && *k <= 1.0 + 1e-12));
        let graded = dissipation(&base.mesh, graded_t, &k_elem);
        assert!(
            graded < 0.75 * solid,
            "graded dissipation {graded} not clearly below the solid {solid}"
        );
    }
}
