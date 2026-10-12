//! The analysis pipeline: document → field → tetrahedral mesh → boundary conditions → P2 solve →
//! nodal results. Runs identically natively (CLI, verification tests) and in a Web Worker.

pub mod fill;
pub mod fsmz;
pub mod import;
pub mod probe;
pub mod regions;
pub mod step;
pub mod steptess;
pub mod voxel;

use fieldsmith_doc::loaddist::LoadFn;
use fieldsmith_doc::{materials, Document, LoadDir, LoadKind};
use fieldsmith_field::expr::{self, Expr};
use fieldsmith_mesh::{stuff, Quality};
use fieldsmith_solve::{
    element_stress, nodal_stress, solve, von_mises, Material, P2Mesh, Problem, SolveReport,
    SolverOptions, Stress,
};
use serde::Serialize;

pub use regions::{face_frame, region_facets, FaceFrame, Facet};

/// Monotonic clock in milliseconds (wall clock natively, `performance.now()` in the browser).
pub type Clock = fn() -> f64;

/// Wall-clock milliseconds, for measuring durations. The `std` clock natively; the JS clock
/// in the browser, because wasm32's std has no clock and panics at `Instant::now`.
pub fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        thread_local! {
            static T0: std::time::Instant = std::time::Instant::now();
        }
        T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
    }
}

/// How fine an analysis to run.
#[derive(Clone, Copy, Debug)]
pub enum Resolution {
    /// Target element size (mm).
    Size(f64),
    /// Aim for about this many degrees of freedom.
    Dofs(usize),
}

#[derive(Clone, Debug, Serialize)]
pub struct CaseResult {
    pub name: String,
    /// Nodal displacement, 3 per node (mm).
    #[serde(skip)]
    pub u: Vec<f64>,
    #[serde(skip)]
    pub stress: Vec<Stress>,
    #[serde(skip)]
    pub von_mises: Vec<f64>,
    /// Von Mises at each element centroid. V3.md: P2 strains extrapolated to element corners
    /// overshoot near curved free edges, so the stress readout is built on the centroid
    /// statistic, not on recovered nodal maxima.
    #[serde(skip)]
    pub vm_elem: Vec<f64>,
    pub max_von_mises: f64,
    pub max_vm_elem: f64,
    pub max_displacement: f64,
    pub solve: SolveReport,
    /// Provenance of each load in this case, in document order: the applied resultant
    /// and moment, and the face-fit numbers of distributed loads. Empty for the
    /// verifier's resolved-model runs (which call `run_case` directly).
    pub load_reports: Vec<LoadReport>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Provenance {
    pub elements: usize,
    pub order: u8,
    pub nodes: usize,
    pub dofs: usize,
    pub element_size: f64,
    pub mesh_ms: f64,
    pub solve_ms: f64,
    pub total_ms: f64,
    pub quality: Quality,
    pub iterations: usize,
}

#[derive(Debug)]
pub struct Analysis {
    pub mesh: P2Mesh,
    pub young: Vec<f64>,
    pub poisson: f64,
    pub yield_strength: f64,
    pub cases: Vec<CaseResult>,
    /// Max over cases of nodal von Mises.
    pub envelope: Vec<f64>,
    /// Max over cases of element-centroid von Mises.
    pub envelope_elem: Vec<f64>,
    /// Boundary triangles of the P2 mesh: 6 node ids each (corners, then midsides 01, 12, 20).
    pub surface: Vec<[u32; 6]>,
    /// Element that owns each surface facet (index into `mesh.tets`), same order as `surface`.
    pub surface_owner: Vec<u32>,
    /// Index into `doc.regions` of the region each surface facet matches, `u32::MAX` for none.
    /// Best effort: a region whose field evaluation fails is left unmatched (`validate()`
    /// reports those); where several regions claim a facet the last one in document order wins.
    pub surface_region: Vec<u32>,
    /// Facet ids matched by each region, blocks in `doc.regions` order. Unlike
    /// `surface_region`, every claimant keeps its set: a facet claimed by several regions
    /// appears in each claiming region's block, so per-region consumers (view glyphs) find
    /// the facets their constraints actually act on.
    pub region_facet_sets: Vec<Vec<u32>>,
    pub provenance: Provenance,
    /// First mode frequencies when a modal run was requested (`analyse_full(…, Some(n))`).
    /// The mesh is shared, so the frequencies come free of extra meshing; the modal
    /// solve's own time is in `ModalResult.solve_ms`, which `provenance` does not include.
    pub modes: Option<fieldsmith_solve::modal::ModalResult>,
}

/// Problems the user can fix, reported in their words.
#[derive(Clone, Debug)]
pub struct SetupError(pub String);

impl std::fmt::Display for SetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn material_of(doc: &Document) -> Result<materials::Material, SetupError> {
    materials::find(doc, &doc.material)
        .ok_or_else(|| SetupError(format!("unknown material '{}'", doc.material)))
}

/// Load case names in document order (loads without a case name share "Load 1").
pub fn case_names(doc: &Document) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for l in &doc.loads {
        if !names.contains(&l.case) {
            names.push(l.case.clone());
        }
    }
    names
}

/// Element size for a target DOF count, from the solid's bounding box and an estimated fill.
/// P2 on a BCC lattice gives about 48·V/h³ DOFs.
pub fn size_for_dofs(volume: f64, dofs: usize) -> f64 {
    (48.0 * volume / dofs as f64).cbrt()
}

/// Von Mises at each element centroid. The centroid of a reference tetrahedron is the average
/// of its four corners, [0.25; 3].
pub fn elem_von_mises(mesh: &P2Mesh, u: &[f64], young: &[f64], poisson: f64) -> Vec<f64> {
    (0..mesh.tets.len())
        .map(|e| von_mises(&element_stress(mesh, u, e, young[e], poisson, [0.25; 3])))
        .collect()
}

/// Support constraints as a per-node bit mask: bit 0/1/2 = x/y/z fixed. Shared by the solid and
/// graded-fill solve paths, and by the verifier's resolved-model runs (same document semantics
/// on a different mesh).
pub fn fixed_mask(
    doc: &Document,
    mesh: &P2Mesh,
    facets: &[Facet],
    h: f64,
) -> Result<Vec<u8>, SetupError> {
    let mut fixed = vec![0u8; mesh.nodes.len()];
    for s in &doc.supports {
        let fs = region_facets(doc, &s.region, mesh, facets, h)?;
        if fs.is_empty() {
            return Err(SetupError(format!(
                "support '{}': region '{}' does not touch the part's surface",
                s.id, s.region
            )));
        }
        let mask = (s.fixed[0] as u8) | (s.fixed[1] as u8) << 1 | (s.fixed[2] as u8) << 2;
        for &fi in &fs {
            for &n in &facets[fi].nodes {
                fixed[n as usize] |= mask;
            }
        }
    }
    Ok(fixed)
}

/// Provenance of one load's application: how the function or vector became nodal loads.
/// Resultant and moment are integrated over the region's facets with the traction
/// actually applied (after any `total=` rescale); moment is about the document origin.
/// `fit_*` numbers exist only for distributed loads; `unscaled`/`scale` are the
/// resultant along the load's own direction before/after rescale (for Force and
/// Pressure loads: the resultant's magnitude, scale always 1).
#[derive(Clone, Debug, Serialize)]
pub struct LoadReport {
    pub id: String,
    pub region: String,
    pub facets: usize,
    pub resultant: [f64; 3],
    pub moment: [f64; 3],
    pub unscaled: f64,
    pub scale: f64,
    pub fit_rms: Option<f64>,
    pub fit_extent: Option<f64>,
}

/// Nodal force vector for one load case, plus the per-load provenance: uniform traction
/// over the case's facets (corners 0, each midside a/3) or a distributed density
/// evaluated at facet centroids. Shared by the solid and graded-fill solve paths, and
/// by the verifier's resolved-model runs.
pub fn case_force_report(
    doc: &Document,
    mesh: &P2Mesh,
    facets: &[Facet],
    name: &str,
    h: f64,
) -> Result<(Vec<f64>, Vec<LoadReport>), SetupError> {
    let mut force = vec![0.0; mesh.nodes.len() * 3];
    let mut reports = Vec::new();
    for l in doc.loads.iter().filter(|l| l.case == name) {
        let fs = region_facets(doc, &l.region, mesh, facets, h)?;
        if fs.is_empty() {
            return Err(SetupError(format!(
                "load '{}': region '{}' does not touch the part's surface",
                l.id, l.region
            )));
        }
        let area: f64 = fs.iter().map(|&i| facets[i].area(mesh)).sum();
        // The distributed arm: compile, fit the face frame, choose the direction, spot
        // a zero or divergent resultant before nodal loads are built.
        /// The distributed arm's pre-computed bits for one load.
        struct Dist {
            d: [f64; 3],
            scale: f64,
            frame: regions::FaceFrame,
            q: Vec<f64>,
            unscaled: f64,
        }
        let dist: Option<Dist> = match &l.kind {
            LoadKind::Distributed { expr, dir, total } => {
                let f = LoadFn::compile(expr)
                    .map_err(|e| SetupError(format!("load '{}': fn: {e}", l.id)))?;
                let frame = face_frame(mesh, facets, &fs);
                if frame.rms > regions::FACE_FRAME_RMS_FRACTION * frame.extent {
                    return Err(SetupError(format!(
                        "load '{}': region '{}' curves away from its fitted plane (fit RMS \
                         {:.3} mm over a {:.3} mm extent, the bound is {:.0} %); label a \
                         flatter patch or set the load with dir=",
                        l.id,
                        l.region,
                        frame.rms,
                        frame.extent,
                        regions::FACE_FRAME_RMS_FRACTION * 100.0
                    )));
                }
                let d = match dir {
                    LoadDir::Global { v } => *v,
                    LoadDir::Local { v } => {
                        [
                            v[0] * frame.x[0] + v[1] * frame.y[0] + v[2] * frame.n[0],
                            v[0] * frame.x[1] + v[1] * frame.y[1] + v[2] * frame.n[1],
                            v[0] * frame.x[2] + v[1] * frame.y[2] + v[2] * frame.n[2],
                        ]
                    }
                };
                let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                if len == 0.0 {
                    return Err(SetupError(format!(
                        "load '{}': the direction is the zero vector",
                        l.id
                    )));
                }
                let d = d.map(|c| c / len);
                // Density at each facet's centroid in face-local coordinates.
                let mut q = Vec::<f64>::with_capacity(fs.len());
                for &fi in &fs {
                    let c = facets[fi].centroid(mesh);
                    let lx = (c[0] - frame.origin[0]) * frame.x[0]
                        + (c[1] - frame.origin[1]) * frame.x[1]
                        + (c[2] - frame.origin[2]) * frame.x[2];
                    let ly = (c[0] - frame.origin[0]) * frame.y[0]
                        + (c[1] - frame.origin[1]) * frame.y[1]
                        + (c[2] - frame.origin[2]) * frame.y[2];
                    let v = f.eval(lx, ly);
                    if !v.is_finite() {
                        return Err(SetupError(format!(
                            "load '{}': the density is not finite at facet {}'s centroid \
                             (x = {:.3}, y = {:.3} mm on the face; world ({:.3}, {:.3}, {:.3}) mm); \
                             the fn diverges or divides by zero there",
                            l.id, fi, lx, ly, c[0], c[1], c[2]
                        )));
                    }
                    q.push(v);
                }
                let unscaled: f64 = fs
                    .iter()
                    .zip(&q)
                    .map(|(&fi, v)| *v * facets[fi].area(mesh))
                    .sum();
                let scale = match total {
                    None => 1.0,
                    Some(target) => {
                        // Near-cancellation: Σ|q|·A dwarfs Σq·A means the function has a
                        // spread it cannot rescale to a stated resultant without an
                        // unbounded factor. Bound: the signed resultant must be at least
                        // 1 % of the density's total absolute weight.
                        let rabs: f64 = fs
                            .iter()
                            .zip(&q)
                            .map(|(&fi, v)| v.abs() * facets[fi].area(mesh))
                            .sum();
                        if *target == 0.0 {
                            return Err(SetupError(format!(
                                "load '{}': total=0 makes the load vanish — give the \
                                 intended resultant in N",
                                l.id
                            )));
                        }
                        if unscaled.abs() < 0.01 * rabs {
                            return Err(SetupError(format!(
                                "load '{id}': total={target} N cannot rescale: the function's \
                                 resultant along the stated direction almost cancels \
                                 ({unscaled} N against {rabs} N of density; the bound \
                                 is 1 %) — write the sign sense into the fn",
                                id = l.id
                            )));
                        }
                        target / unscaled
                    }
                };
                Some(Dist {
                    d,
                    scale,
                    frame,
                    q,
                    unscaled,
                })
            }
            _ => None,
        };
        let mut resultant = [0.0f64; 3];
        let mut moment = [0.0f64; 3];
        for (i, &fi) in fs.iter().enumerate() {
            let fct = &facets[fi];
            let a = fct.area(mesh);
            let t = match &dist {
                Some(dz) => {
                    let v = dz.q[i] * dz.scale;
                    dz.d.map(|c| v * c)
                }
                None => match l.kind {
                    LoadKind::Force { vector } => vector.map(|c| c / area),
                    LoadKind::Pressure { mpa } => fct.normal(mesh).map(|c| -mpa * c),
                    LoadKind::Distributed { .. } => unreachable!("dist is Some here"),
                },
            };
            // Uniform traction on a 6-node triangle: corners 0, each midside a/3.
            for &n in &fct.nodes[3..] {
                for c in 0..3 {
                    force[3 * n as usize + c] += t[c] * a / 3.0;
                }
                let _ = n;
            }
            for c in 0..3 {
                resultant[c] += t[c] * a;
            }
            // Moment of the traction about the document origin: r × F_t.
            let c = fct.centroid(mesh);
            let f3 = t.map(|v| v * a);
            let m = [
                c[1] * f3[2] - c[2] * f3[1],
                c[2] * f3[0] - c[0] * f3[2],
                c[0] * f3[1] - c[1] * f3[0],
            ];
            for k in 0..3 {
                moment[k] += m[k];
            }
        }
        let report = match &dist {
            Some(dz) => LoadReport {
                id: l.id.clone(),
                region: l.region.clone(),
                facets: fs.len(),
                resultant,
                moment,
                unscaled: dz.unscaled,
                scale: dz.scale,
                fit_rms: Some(dz.frame.rms),
                fit_extent: Some(dz.frame.extent),
            },
            None => {
                let mag =
                    (resultant[0].powi(2) + resultant[1].powi(2) + resultant[2].powi(2)).sqrt();
                LoadReport {
                    id: l.id.clone(),
                    region: l.region.clone(),
                    facets: fs.len(),
                    resultant,
                    moment,
                    unscaled: mag,
                    scale: 1.0,
                    fit_rms: None,
                    fit_extent: None,
                }
            }
        };
        reports.push(report);
    }
    Ok((force, reports))
}

/// Nodal force vector for one load case. Verifier-resolved runs and simple callers use
/// this; the analyse and graded paths use `case_force_report` for provenance.
pub fn case_force(
    doc: &Document,
    mesh: &P2Mesh,
    facets: &[Facet],
    name: &str,
    h: f64,
) -> Result<Vec<f64>, SetupError> {
    case_force_report(doc, mesh, facets, name, h).map(|(f, _)| f)
}

/// Solve one case and recover per-node and per-element von Mises with the given per-element
/// moduli. Shared by the solid and graded-fill solve paths, and by the verifier's resolved-model
/// runs.
pub fn run_case(
    mesh: &P2Mesh,
    young: &[f64],
    poisson: f64,
    fixed: &[u8],
    force: Vec<f64>,
    name: String,
) -> Result<CaseResult, SetupError> {
    let problem = Problem {
        mesh: mesh.clone(),
        material: Material {
            young: young.to_vec(),
            poisson,
        },
        fixed: fixed.to_vec(),
        force,
    };
    let (u, rep) = solve(&problem, SolverOptions::default()).map_err(SetupError)?;
    let stress = nodal_stress(mesh, &u, young, poisson);
    let vm: Vec<f64> = stress.iter().map(von_mises).collect();
    let vm_elem = elem_von_mises(mesh, &u, young, poisson);
    let max_d = (0..mesh.nodes.len())
        .map(|i| (u[3 * i].powi(2) + u[3 * i + 1].powi(2) + u[3 * i + 2].powi(2)).sqrt())
        .fold(0.0, f64::max);
    Ok(CaseResult {
        name,
        max_von_mises: vm.iter().cloned().fold(0.0, f64::max),
        max_vm_elem: vm_elem.iter().cloned().fold(0.0, f64::max),
        max_displacement: max_d,
        u,
        stress,
        von_mises: vm,
        vm_elem,
        solve: rep,
        load_reports: Vec::new(),
    })
}

/// The meshing and constraint stage of the pipeline, shared by the static (`analyse_full`)
/// and modal (`analyse_modes`) paths: field → resolution size → voxel mesh → P2 mesh +
/// boundary facets → region maps → support mask. `need_loads` selects the static path's
/// "add a load" rule; the modal path needs supports only.
struct Prepared {
    mesh: P2Mesh,
    facets: Vec<Facet>,
    surface: Vec<[u32; 6]>,
    surface_owner: Vec<u32>,
    surface_region: Vec<u32>,
    region_facet_sets: Vec<Vec<u32>>,
    fixed: Vec<u8>,
    mat: materials::Material,
    young: Vec<f64>,
    h: f64,
    quality: Quality,
    mesh_ms: f64,
}

fn prepare(
    doc: &Document,
    res: Resolution,
    clock: Clock,
    need_loads: bool,
) -> Result<Prepared, SetupError> {
    let t0 = clock();
    let body = expr::compile_body(doc);
    let b = expr::bounds(&body);
    if b.is_empty() {
        return Err(SetupError("the model is empty".into()));
    }
    if doc.supports.is_empty() {
        return Err(SetupError(
            "add a support: the part needs something to hold it".into(),
        ));
    }
    if need_loads && doc.loads.is_empty() {
        return Err(SetupError("add a load".into()));
    }
    let mat = material_of(doc)?;
    let h = match res {
        Resolution::Size(h) => h,
        Resolution::Dofs(n) => {
            let v = fieldsmith_field::volume::integrate(
                &body,
                fieldsmith_field::volume::default_spacing(&b) * 2.0,
                None,
            );
            size_for_dofs(v.max(1e-9), n)
        }
    };
    let f = |p: [f64; 3]| expr::eval(&body, p).v;
    let (mut lin, _) = stuff(f, b.lo, b.hi, h);
    lin.compact();
    if lin.tets.is_empty() {
        return Err(SetupError(
            "the part is too thin to mesh at this resolution".into(),
        ));
    }
    let quality = fieldsmith_mesh::quality(&lin);
    let mesh = P2Mesh::from_linear(&lin.verts, &lin.tets);
    let boundary: Vec<([u32; 3], u32)> = lin.boundary();
    let t_mesh = clock();

    // Boundary faces as P2 triangles (and which tet owns them).
    let facets: Vec<Facet> = boundary
        .iter()
        .map(|(f, t)| {
            let tet = &mesh.tets[*t as usize];
            let local = f.map(|v| {
                tet[..4]
                    .iter()
                    .position(|&x| x == v)
                    .expect("face vertex in tet")
            });
            Facet {
                nodes: fieldsmith_solve::p2::face_nodes(tet, local),
            }
        })
        .collect();
    let surface: Vec<[u32; 6]> = facets.iter().map(|f| f.nodes).collect();
    let surface_owner: Vec<u32> = boundary.iter().map(|(_, t)| *t).collect();
    let mut surface_region = vec![u32::MAX; surface.len()];
    let mut region_facet_sets: Vec<Vec<u32>> = vec![Vec::new(); doc.regions.len()];
    for (ri, r) in doc.regions.iter().enumerate() {
        if let Ok(fs) = region_facets(doc, &r.id, &mesh, &facets, h) {
            region_facet_sets[ri] = fs.iter().map(|&i| i as u32).collect();
            for fi in fs {
                surface_region[fi] = ri as u32;
            }
        }
    }

    // Supports.
    let fixed = fixed_mask(doc, &mesh, &facets, h)?;
    let young = vec![mat.e; mesh.tets.len()];
    Ok(Prepared {
        mesh,
        facets,
        surface,
        surface_owner,
        surface_region,
        region_facet_sets,
        fixed,
        mat,
        young,
        h,
        quality,
        mesh_ms: t_mesh - t0,
    })
}

/// Modal analysis (M8): the same meshing, material and region-based supports as `analyse`,
/// without loads — mode frequencies need the part held, not pushed. Solves the first
/// `count` modes on the document resolution's mesh. Modal-only callers (CLI `--modes`,
/// V13) get this; `analyse_full(…, Some(count))` appends the same run to the static
/// analysis on the shared mesh.
#[derive(Clone, Debug, Serialize)]
pub struct ModalRun {
    /// The mesh's provenance (elements, DOFs, quality, mesh time). `iterations` stays 0:
    /// the eigen route reports no iteration counts.
    pub provenance: Provenance,
    pub modal: fieldsmith_solve::modal::ModalResult,
}

pub fn analyse_modes(
    doc: &Document,
    res: Resolution,
    clock: Clock,
    count: usize,
) -> Result<ModalRun, SetupError> {
    let Prepared {
        mesh,
        fixed,
        mat,
        young,
        h,
        quality,
        mesh_ms,
        ..
    } = prepare(doc, res, clock, false)?;
    // Density g/cm³ → N·s²/mm⁴ (the modal solve's units, mm-N-s).
    let t_modal = clock();
    let mut modal = fieldsmith_solve::solve_modes(
        &mesh,
        &young,
        mat.nu,
        mat.density * 1e-9,
        &fixed,
        &fieldsmith_solve::ModeOptions {
            count,
            ..Default::default()
        },
    )
    .map_err(SetupError)?;
    // The solver runs clock-free (std clocks panic on wasm32); the caller times it.
    modal.solve_ms = clock() - t_modal;
    Ok(ModalRun {
        provenance: Provenance {
            elements: mesh.tets.len(),
            order: 2,
            nodes: mesh.nodes.len(),
            dofs: mesh.nodes.len() * 3,
            element_size: h,
            mesh_ms,
            solve_ms: modal.solve_ms,
            total_ms: mesh_ms + modal.solve_ms,
            quality,
            iterations: 0,
        },
        modal,
    })
}

/// Full analysis: static cases, plus — when `modes` is set — the first mode frequencies
/// solved on the same mesh (one meshing serves both). `analyse` is the static-only case.
pub fn analyse_full(
    doc: &Document,
    res: Resolution,
    clock: Clock,
    modes: Option<usize>,
) -> Result<Analysis, SetupError> {
    let Prepared {
        mesh,
        facets,
        surface,
        surface_owner,
        surface_region,
        region_facet_sets,
        fixed,
        mat,
        young,
        h,
        quality,
        mesh_ms,
    } = prepare(doc, res, clock, true)?;
    // `prepare` has just returned, so `clock` is its meshing end; its t0 is mesh_ms back.
    let t_mesh = clock();
    let t0 = t_mesh - mesh_ms;
    let nn = mesh.nodes.len();
    let mut cases = Vec::new();
    let mut iterations = 0;
    for name in case_names(doc) {
        let (force, reports) = case_force_report(doc, &mesh, &facets, &name, h)?;
        let mut case = run_case(&mesh, &young, mat.nu, &fixed, force, name)?;
        case.load_reports = reports;
        iterations += case.solve.iterations;
        cases.push(case);
    }
    let envelope = (0..nn)
        .map(|i| cases.iter().map(|c| c.von_mises[i]).fold(0.0, f64::max))
        .collect();
    let envelope_elem = (0..mesh.tets.len())
        .map(|e| cases.iter().map(|c| c.vm_elem[e]).fold(0.0, f64::max))
        .collect();
    let t_static_end = clock();
    // Density g/cm³ → N·s²/mm⁴ (the modal solve's units, mm-N-s).
    let modal = match modes {
        None => None,
        Some(count) => {
            // The solver runs clock-free (std clocks panic on wasm32); the caller times it.
            let t_modal = clock();
            let mut m = fieldsmith_solve::solve_modes(
                &mesh,
                &young,
                mat.nu,
                mat.density * 1e-9,
                &fixed,
                &fieldsmith_solve::ModeOptions {
                    count,
                    ..Default::default()
                },
            )
            .map_err(SetupError)?;
            m.solve_ms = clock() - t_modal;
            Some(m)
        }
    };
    let provenance = Provenance {
        elements: mesh.tets.len(),
        order: 2,
        nodes: nn,
        dofs: nn * 3,
        element_size: h,
        mesh_ms,
        solve_ms: t_static_end - t_mesh,
        total_ms: t_static_end - t0,
        quality,
        iterations,
    };
    Ok(Analysis {
        mesh,
        young,
        poisson: mat.nu,
        yield_strength: mat.yield_strength,
        cases,
        envelope,
        envelope_elem,
        surface,
        surface_owner,
        surface_region,
        region_facet_sets,
        provenance,
        modes: modal,
    })
}

/// The document-driven entry: static cases, plus the first `doc.solve.modes` mode
/// frequencies when the document asks for a modal run. (Explicit callers use
/// [`analyse_full`].)
pub fn analyse(doc: &Document, res: Resolution, clock: Clock) -> Result<Analysis, SetupError> {
    analyse_full(doc, res, clock, doc.solve.modes)
}

/// The compiled body field (for callers that need to evaluate it alongside an analysis).
pub fn body(doc: &Document) -> Expr {
    expr::compile_body(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldsmith_doc::fsm;

    fn parse(src: &str) -> Document {
        fsm::parse(&format!("fieldsmith 1\n{src}")).expect("valid test document")
    }

    fn now() -> f64 {
        use std::time::Instant;
        thread_local!(static T0: Instant = Instant::now());
        T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
    }

    /// Two regions claiming the same face: the shared map keeps the last claim, while each
    /// region's own facet set keeps the facets it actually matched.
    const OVERLAP: &str = "\
name \"Region overlap\"
custommaterial steel200 e=200000 nu=0.3 yield=250 density=7.85
material steel200
box bar size=(40, 10, 10) at=(20, 5, 5)
region one = bar.px
region two = bar.px
support clamp one fixed=xyz
load pull two force=(100, 0, 0)
";

    #[test]
    fn region_overlap_keeps_per_region_facet_sets() {
        let d = parse(OVERLAP);
        let a = analyse(&d, Resolution::Size(1.5), now).expect("analyse");
        let one = &a.region_facet_sets[0];
        let two = &a.region_facet_sets[1];
        assert!(!one.is_empty(), "region 'one' matched no facets");
        assert_eq!(one, two, "both regions claim the same face");

        // The shared map's last claim wins, and every claimed surface facet is exactly one of
        // the two sets.
        for (fi, &ri) in a.surface_region.iter().enumerate() {
            assert!(
                ri == 0 || ri == 1 || ri == u32::MAX,
                "facet {fi} mapped to region {ri}"
            );
            if ri != u32::MAX {
                assert!(
                    a.region_facet_sets[ri as usize].contains(&(fi as u32)),
                    "facet {fi} maps to region {ri} but its set lacks it"
                );
            }
        }
    }

    /// Modal on an unloaded cantilever (M8): supports alone define the constraints, so a
    /// document without loads still produces frequencies, ascending.
    const FREE: &str = "\
name \"Modal bar\"
custommaterial steel e=200000 nu=0.3 yield=250 density=7.85
material steel
box bar size=(40, 10, 10) at=(20, 5, 5)
region root = bar.px
support clamp root fixed=xyz
";

    #[test]
    fn modal_runs_without_loads_and_frequencies_ascend() {
        let d = parse(FREE);
        let run = analyse_modes(&d, Resolution::Size(1.5), now, 4).expect("modal run");
        let fs = &run.modal.frequencies;
        assert_eq!(fs.len(), 4);
        assert!(fs.iter().all(|f| *f > 0.0 && f.is_finite()));
        assert!(
            fs.windows(2).all(|w| w[0] <= w[1]),
            "frequencies descending: {fs:?}"
        );
        // The static path through the same document still wants a load.
        let err = analyse_full(&d, Resolution::Size(1.5), now, None).expect_err("no loads");
        assert_eq!(err.0, "add a load");
        // And with a load, the same document carries the modes on the shared mesh.
        let doc = parse(OVERLAP);
        let a = analyse_full(&doc, Resolution::Size(3.0), now, Some(2)).expect("analyse");
        let modes = a.modes.as_ref().expect("modes");
        assert_eq!(modes.frequencies.len(), 2);
        assert!(modes.frequencies.iter().all(|f| *f > 0.0));
        // provenance stays the static pipeline's: mesh + static solves (no modal time).
        assert!(
            (a.provenance.total_ms - a.provenance.mesh_ms - a.provenance.solve_ms).abs() < 50.0,
            "total {} ≠ mesh {} + solve {}",
            a.provenance.total_ms,
            a.provenance.mesh_ms,
            a.provenance.solve_ms
        );
    }
}
