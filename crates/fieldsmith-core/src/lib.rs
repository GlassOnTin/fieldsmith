//! The analysis pipeline: document → field → tetrahedral mesh → boundary conditions → P2 solve →
//! nodal results. Runs identically natively (CLI, verification tests) and in a Web Worker.

pub mod fill;
pub mod fsmz;
pub mod import;
pub mod probe;
pub mod regions;
pub mod rung;
pub mod step;
pub mod steptess;
pub mod voxel;

use fieldsmith_doc::loaddist::LoadFn;
use fieldsmith_doc::{materials, Document, LoadDir, LoadKind};
use fieldsmith_field::expr::{self, Expr};
use fieldsmith_mesh::{stuff, Quality};
use fieldsmith_solve::{
    compliance, element_stress, element_stress_thermal, nodal_stress, nodal_stress_thermal, solve,
    solve_conduction, thermal_load, upper_compliance, von_mises, HeatFluxBc, Material, P2Mesh,
    Problem, RobinBc, SolveReport, SolverOptions, Stress, ThermalRefData, V3,
};
use serde::Serialize;

pub use regions::{face_frame, region_facets, region_facets_exact, FaceFrame, Facet};

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
    /// The raw-force compliance fᵀu of the load vector the solve actually saw:
    /// mechanical loads on a mechanical case, the thermal-expansion pseudo-load on a
    /// thermal case. By Galerkin orthogonality this is a(u_h, u_h) ≥ 0 for a converged
    /// solve, and a certified lower bound C_h ≤ C_true on the exact compliance —
    /// strict when the exact solution leaves the P2 span, exact when the mesh
    /// represents it. Meaningful only with `solve.converged`
    /// (fieldsmith-solve's `bounds` tests pin the theorem).
    pub compliance: f64,
    /// The recovered-stress upper bound C(σ*) (N·mm) when the equilibration ran; absent
    /// from the canon and the wire unless the caller asked for it. Certified up to the
    /// P2 interpolation defect (crates/fieldsmith-solve/src/dual.rs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compliance_upper: Option<f64>,
    pub solve: SolveReport,
    /// Provenance of each load in this case, in document order: the applied resultant
    /// and moment, and the face-fit numbers of distributed loads. Empty for the
    /// verifier's resolved-model runs (which call `run_case` directly), and for
    /// thermal cases (whose driver is the conduction field, not a load).
    pub load_reports: Vec<LoadReport>,
    /// Nodal temperature (°C) for a thermal case — the conduction field, rendered with
    /// a legend in the Results lens. `None` on mechanical cases.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<Vec<f64>>,
    /// The conduction solve's report for a thermal case (`solve` is the expansion
    /// solve's own report). `None` on mechanical cases.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conduction: Option<SolveReport>,
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
    /// Time in the thermal cases' conduction and expansion solves; 0.0 for a document
    /// without temperature statements. `total_ms` includes it.
    pub thermal_ms: f64,
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

/// [`elem_von_mises`] for a thermal case: the elastic stress with the thermal term
/// subtracted ([`element_stress_thermal`]), ΔT at the centroid evaluated from the nodal
/// temperature field.
pub fn elem_von_mises_thermal(
    mesh: &P2Mesh,
    u: &[f64],
    young: &[f64],
    poisson: f64,
    alpha: f64,
    dt: &[f64],
) -> Vec<f64> {
    use fenics_element::{FiniteElement, P2Tetrahedron};
    // The P2 basis at the reference centroid [1/4, 1/4, 1/4]: −1/8 on each vertex,
    // +1/4 on each midside (sums to 1 — the partition-checked interpolation weights).
    let phi = P2Tetrahedron.evaluate_basis(&[0.25, 0.25, 0.25]);
    (0..mesh.tets.len())
        .map(|e| {
            let t = &mesh.tets[e];
            let dt_c: f64 = t
                .iter()
                .zip(phi.iter())
                .map(|(&n, p)| dt[n as usize] * p)
                .sum();
            let s = element_stress_thermal(mesh, u, e, young[e], poisson, alpha, dt_c, [0.25; 3]);
            von_mises(&s)
        })
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
        // Exact-only: a support pin must land on nodes the field agrees are immovable; the
        // second pass pins off-carrier chime nodes and contradicts the exact solution (V20).
        let fs = region_facets_exact(doc, &s.region, mesh, facets, h)?;
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
/// The load builder's output: the nodal force vector, the per-load reports, and the
/// prescribed per-facet constant tractions (keyed by the exact f64 bits of the facet's
/// three corner node positions, sorted) — the recovery's boundary classification data.
pub type CaseLoads = (
    Vec<f64>,
    Vec<LoadReport>,
    std::collections::HashMap<[u64; 9], V3>,
);

pub fn case_force_report(
    doc: &Document,
    mesh: &P2Mesh,
    facets: &[Facet],
    name: &str,
    h: f64,
) -> Result<CaseLoads, SetupError> {
    let mut force = vec![0.0; mesh.nodes.len() * 3];
    let mut reports = Vec::new();
    let mut prescribed: std::collections::HashMap<[u64; 9], V3> = std::collections::HashMap::new();
    for l in doc.loads.iter().filter(|l| l.case == name) {
        // Exact-only: traction integrates over the claimed facets' real area, and chime
        // facets the second pass would add (with their mesh-chord area) belong to no
        // carrier the load names; loads are data at exact carriers like constraints are.
        let fs = region_facets_exact(doc, &l.region, mesh, facets, h)?;
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
                    LoadDir::Local { v } => [
                        v[0] * frame.x[0] + v[1] * frame.y[0] + v[2] * frame.n[0],
                        v[0] * frame.x[1] + v[1] * frame.y[1] + v[2] * frame.n[1],
                        v[0] * frame.x[2] + v[1] * frame.y[2] + v[2] * frame.n[2],
                    ],
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
            // The facet's constant traction, recorded for the certified recovery's
            // boundary classification (exact-f64 bit key of the corner node positions;
            // the nodes are shared with the solve's mesh, so the bits are identical).
            let mut key: Vec<u64> = fct.nodes[..3]
                .iter()
                .map(|&n| mesh.nodes[n as usize])
                .flat_map(|p| p.map(|c| c.to_bits()))
                .collect();
            key.sort_unstable();
            let mut key2 = [0u64; 9];
            key2.copy_from_slice(&key);
            prescribed.insert(key2, t);
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
    Ok((force, reports, prescribed))
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
    case_force_report(doc, mesh, facets, name, h).map(|(f, _, _)| f)
}

/// Nodal Dirichlet map for one thermal case: every P2 node on a temperature statement's
/// facets holds the statement's value; nodes no statement reaches stay insulated (the
/// natural boundary condition of the Laplace solve). A region that does not touch the
/// surface, and two statements in one case meeting on a shared surface node with
/// different values, are named setup errors — a node cannot carry both temperatures.
pub fn case_temperatures(
    doc: &Document,
    mesh: &P2Mesh,
    facets: &[Facet],
    name: &str,
    h: f64,
) -> Result<Vec<Option<f64>>, SetupError> {
    let mut d: Vec<Option<f64>> = vec![None; mesh.nodes.len()];
    let mut owner: Vec<Option<String>> = vec![None; mesh.nodes.len()];
    for t in doc.temperatures.iter().filter(|t| t.case == name) {
        let fs = region_facets(doc, &t.region, mesh, facets, h)?;
        if fs.is_empty() {
            return Err(SetupError(format!(
                "temperature '{}': region '{}' does not touch the part's surface",
                t.id, t.region
            )));
        }
        for &fi in &fs {
            for &n in &facets[fi].nodes {
                let n = n as usize;
                if let (Some(prev), Some(prev_v)) = (&owner[n], d[n]) {
                    if prev != &t.id && prev_v != t.celsius {
                        return Err(SetupError(format!(
                            "temperatures '{prev}' and '{}' meet at shared surface nodes \
                             but hold different values ({prev_v} °C and {} °C) — a node \
                             cannot carry both temperatures in one thermal case",
                            t.id, t.celsius
                        )));
                    }
                }
                owner[n] = Some(t.id.clone());
                d[n] = Some(t.celsius);
            }
        }
    }
    Ok(d)
}

/// Robin bands for one thermal case: every convection statement named for the case
/// resolves its region to surface facets ([`RobinBc`]). A region that does not touch the
/// surface is a setup error, the same naming as temperatures. Temperature statements may
/// sit on the same regions — their Dirichlet lift overrides the convective values there,
/// which is the intended hot-wall-plus-coolant combination.
pub fn case_convections(
    doc: &Document,
    mesh: &P2Mesh,
    facets: &[Facet],
    name: &str,
    h: f64,
) -> Result<Vec<RobinBc>, SetupError> {
    let mut bands = Vec::new();
    for c in doc.convections.iter().filter(|c| c.case == name) {
        let fs = region_facets(doc, &c.region, mesh, facets, h)?;
        if fs.is_empty() {
            return Err(SetupError(format!(
                "convection '{}': region '{}' does not touch the part's surface",
                c.id, c.region
            )));
        }
        bands.push(RobinBc {
            h: c.h,
            t_inf: c.t_inf,
            facets: fs.iter().map(|&i| facets[i].nodes).collect(),
        });
    }
    Ok(bands)
}

/// Prescribed heat-flux bands for one thermal case: every heatflux statement named for
/// the case resolves its region to surface facets ([`HeatFluxBc`]), the same naming as
/// convections. Flux bands only add to the right-hand side, so two of them on one
/// region superpose legally — unlike convections, there is no per-region value to
/// conflict.
pub fn case_heat_fluxes(
    doc: &Document,
    mesh: &P2Mesh,
    facets: &[Facet],
    name: &str,
    h: f64,
) -> Result<Vec<HeatFluxBc>, SetupError> {
    let mut bands = Vec::new();
    for c in doc.heat_fluxes.iter().filter(|c| c.case == name) {
        let fs = region_facets(doc, &c.region, mesh, facets, h)?;
        if fs.is_empty() {
            return Err(SetupError(format!(
                "heat flux '{}': region '{}' does not touch the part's surface",
                c.id, c.region
            )));
        }
        bands.push(HeatFluxBc {
            q: c.q,
            facets: fs.iter().map(|&i| facets[i].nodes).collect(),
        });
    }
    Ok(bands)
}

/// Solve one case and recover per-node and per-element von Mises with the given per-element
/// moduli. Shared by the solid and graded-fill solve paths, and by the verifier's resolved-model
/// runs.
#[allow(clippy::too_many_arguments)] // the one case call: mesh, moduli, holds, load, name, and the equilibration's inputs
pub fn run_case(
    mesh: &P2Mesh,
    young: &[f64],
    poisson: f64,
    fixed: &[u8],
    force: Vec<f64>,
    name: String,
    equilibrate: bool,
    prescribed: &dyn Fn([V3; 3]) -> Option<V3>,
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
    let compliance_upper = if equilibrate {
        Some(
            upper_compliance(&problem, prescribed)
                .map_err(SetupError)?
                .j,
        )
    } else {
        None
    };
    Ok(CaseResult {
        name,
        max_von_mises: vm.iter().cloned().fold(0.0, f64::max),
        max_vm_elem: vm_elem.iter().cloned().fold(0.0, f64::max),
        max_displacement: max_d,
        compliance: compliance(&problem.force, &u),
        compliance_upper,
        u,
        stress,
        von_mises: vm,
        vm_elem,
        solve: rep,
        load_reports: Vec::new(),
        temperature: None,
        conduction: None,
    })
}

/// Solve one thermal case: steady conduction from the case's temperature regions,
/// convection bands, and prescribed heat fluxes ([`solve_conduction`]), then the elastic
/// solve driven by the
/// thermal-expansion load from the document's reference temperature (`solve tref=`,
/// default 20 °C). Stress recovery carries the thermal term; the nodal temperature and
/// the conduction report travel on the result. A thermal case runs without mechanical
/// loads — mixed cases are named for later. Shared by the static path and the
/// verifier's resolved-model runs. `k` is one conductivity per element (all-ones = the
/// pinned unit solid); the graded path passes the fill's per-element map.
#[allow(clippy::too_many_arguments)]
pub fn run_thermal_case(
    doc: &Document,
    mesh: &P2Mesh,
    young: &[f64],
    poisson: f64,
    alpha: f64,
    fixed: &[u8],
    facets: &[Facet],
    name: String,
    h: f64,
    k: &[f64],
    equilibrate: bool,
) -> Result<CaseResult, SetupError> {
    let d = case_temperatures(doc, mesh, facets, &name, h)?;
    let robin = case_convections(doc, mesh, facets, &name, h)?;
    let flux = case_heat_fluxes(doc, mesh, facets, &name, h)?;
    let (t, creport) = solve_conduction(mesh, &d, k, &robin, &flux, SolverOptions::default())
        .map_err(SetupError)?;
    // ε_th = α·(T − t_ref): the reference temperature is the stress-free one.
    let t_ref = doc.solve.t_ref.unwrap_or(20.0);
    let dt: Vec<f64> = t.iter().map(|ti| ti - t_ref).collect();
    let force = thermal_load(mesh, &dt, young, poisson, alpha, &ThermalRefData::new());
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
    let stress = nodal_stress_thermal(mesh, &u, young, poisson, alpha, &dt);
    let vm: Vec<f64> = stress.iter().map(von_mises).collect();
    let vm_elem = elem_von_mises_thermal(mesh, &u, young, poisson, alpha, &dt);
    let max_d = (0..mesh.nodes.len())
        .map(|i| (u[3 * i].powi(2) + u[3 * i + 1].powi(2) + u[3 * i + 2].powi(2)).sqrt())
        .fold(0.0, f64::max);
    Ok(CaseResult {
        name,
        max_von_mises: vm.iter().cloned().fold(0.0, f64::max),
        max_vm_elem: vm_elem.iter().cloned().fold(0.0, f64::max),
        max_displacement: max_d,
        compliance: compliance(&problem.force, &u),
        compliance_upper: if equilibrate {
            Some(
                upper_compliance(&problem, &|_| Some([0.0, 0.0, 0.0]))
                    .map_err(SetupError)?
                    .j,
            )
        } else {
            None
        },
        u,
        stress,
        von_mises: vm,
        vm_elem,
        solve: rep,
        load_reports: Vec::new(),
        temperature: Some(t),
        conduction: Some(creport),
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
    if need_loads
        && doc.loads.is_empty()
        && doc.temperatures.is_empty()
        && doc.convections.is_empty()
        && doc.heat_fluxes.is_empty()
    {
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
            thermal_ms: 0.0,
            total_ms: mesh_ms + modal.solve_ms,
            quality,
            iterations: 0,
        },
        modal,
    })
}

/// The recovery's boundary-classification closure from the fixed mask and the load
/// builder's prescribed per-facet tractions: a loaded facet keeps its constant traction,
/// a supported side's reaction is free, every other boundary facet is traction-free.
/// Node positions key by exact f64 bits (the nodes are the solve's mesh).
pub(crate) fn prescribed_fn<'a>(
    node_key: &'a std::collections::HashMap<[u64; 3], u8>,
    loaded: &'a std::collections::HashMap<[u64; 9], V3>,
) -> impl Fn([V3; 3]) -> Option<V3> + 'a {
    move |ps: [V3; 3]| {
        let mut key: Vec<u64> = ps.iter().flat_map(|p| p.map(|c| c.to_bits())).collect();
        key.sort_unstable();
        let mut key2 = [0u64; 9];
        key2.copy_from_slice(&key);
        match loaded.get(&key2) {
            Some(t) => Some(*t),
            None => {
                // No prescribed traction: a facet on a supported side is Dirichlet
                // (its reaction is free), every other boundary facet is traction-free.
                let pinned = ps.iter().any(|p| {
                    node_key
                        .get(&[p[0].to_bits(), p[1].to_bits(), p[2].to_bits()])
                        .is_some_and(|&m| m != 0)
                });
                if pinned {
                    None
                } else {
                    Some([0.0, 0.0, 0.0])
                }
            }
        }
    }
}

/// Full analysis: static cases, plus — when `modes` is set — the first mode frequencies
/// solved on the same mesh (one meshing serves both). `analyse` is the static-only case.
pub fn analyse_full(
    doc: &Document,
    res: Resolution,
    clock: Clock,
    modes: Option<usize>,
    equilibrate: bool,
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
        let (force, reports, prescribed_map) = case_force_report(doc, &mesh, &facets, &name, h)?;
        let mut node_key: std::collections::HashMap<[u64; 3], u8> =
            std::collections::HashMap::new();
        for (i, p) in mesh.nodes.iter().enumerate() {
            node_key.insert([p[0].to_bits(), p[1].to_bits(), p[2].to_bits()], fixed[i]);
        }
        let prescribed = prescribed_fn(&node_key, &prescribed_map);
        let mut case = run_case(
            &mesh,
            &young,
            mat.nu,
            &fixed,
            force,
            name,
            equilibrate,
            &prescribed,
        )?;
        case.load_reports = reports;
        iterations += case.solve.iterations;
        cases.push(case);
    }
    let t_static_end = clock();
    // Thermal cases (M14): per case a conduction solve then the expansion solve, on the
    // same mesh and supports. Their results run beside the mechanical ones.
    let mut thermal_ms = 0.0;
    let thermal_cases = doc.thermal_cases();
    if !thermal_cases.is_empty() {
        let alpha = mat.alpha.ok_or_else(|| {
            SetupError(format!(
                "The thermal flow needs a thermal-expansion coefficient and material '{}' \
                 states none — pick a material whose sheet states a CTE, or give a custom \
                 material an a= value",
                doc.material
            ))
        })?;
        let t_thermal = clock();
        // Unit conductivity: the static path's conduction semantics stay the pinned
        // k ≡ 1 solve; the graded re-resolve (fill.rs) passes the core's per-element map.
        let ones_k = vec![1.0; mesh.tets.len()];
        for name in thermal_cases {
            // Thermal cases prescribe no boundary traction; the expansion pseudo-load is
            // the body, the supports' reaction tractions are free.
            let case = run_thermal_case(
                doc,
                &mesh,
                &young,
                mat.nu,
                alpha,
                &fixed,
                &facets,
                name,
                h,
                &ones_k,
                equilibrate,
            )?;
            iterations += case.solve.iterations;
            cases.push(case);
        }
        thermal_ms = clock() - t_thermal;
    }
    // Envelope over every case — mechanical and thermal: the tracked readout is the
    // worst stress the part can see from any of them.
    let envelope = (0..nn)
        .map(|i| cases.iter().map(|c| c.von_mises[i]).fold(0.0, f64::max))
        .collect();
    let envelope_elem = (0..mesh.tets.len())
        .map(|e| cases.iter().map(|c| c.vm_elem[e]).fold(0.0, f64::max))
        .collect();
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
        thermal_ms,
        total_ms: t_static_end - t0 + thermal_ms,
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
    analyse_full(doc, res, clock, doc.solve.modes, false)
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

    /// A mechanical case's compliance readout (the solver-side bound): fᵀu of the load
    /// vector the solve actually saw — here the 10 N pull. The pipelined mesh's surface
    /// nodes carry distance-field noise, so the native assert stakes a band around the
    /// exact c²·V/E; exactness is pinned to solver precision in fieldsmith-solve's
    /// bounds tests.
    const PULL: &str = "\
name \"Pulled bar\"
material petg
box beam size=(40, 10, 10) at=(20, 5, 5)
region pin = beam.nx
region rest = beam.ny
region base = beam.nz
region pull = beam.px
support pin_x pin fixed=x
support rest_y rest fixed=y
support base_z base fixed=z
load pull pull force=(10, 0, 0)
";

    #[test]
    fn case_compliance_is_the_bound_on_the_raw_load() {
        let d = parse(PULL);
        let young = materials::find(&d, "petg").expect("petg").e;
        // c = F/A = 10 N over the 100 mm² px face = 0.1 MPa; the uniform-tension
        // compliance is c²·V/E = F²·L/(E·A).
        let want = 10.0 * 10.0 * 40.0 / (young * 100.0);
        let a = analyse(&d, Resolution::Size(2.0), now).expect("analyse");
        let case = a
            .cases
            .iter()
            .find(|c| c.name == "Load 1")
            .expect("load case");
        assert!(case.solve.converged, "the bound presumes a converged solve");
        // The load the solve saw: 10 N on x (the provenance report's own resultant).
        assert!(
            (case.load_reports[0].resultant[0] - 10.0).abs() < 1e-9,
            "resultant {:?}",
            case.load_reports[0].resultant
        );
        assert!(case.compliance > 0.0, "compliance is not positive");
        // Without the equilibration the upper end is absent from the case and the wire.
        assert!(case.compliance_upper.is_none(), "absent by default");

        assert!(
            (case.compliance - want).abs() < 0.02 * want,
            "compliance {} vs c²·V/E {}",
            case.compliance,
            want
        );
        // Peak pull-direction displacement ≈ c·L/E, through the same surface noise.
        let ux = (0..a.mesh.nodes.len())
            .map(|i| case.u[3 * i])
            .fold(0.0f64, f64::max);
        let want_ux = 10.0 * 40.0 / (100.0 * young);
        println!(
            "  PULL: E = {young}, c²·V/E = {want:.8}, compliance = {:.8}, peak u_x = {ux:.4e} vs c·L/E = {want_ux:.4e}",
            case.compliance
        );
        assert!(
            (ux - want_ux).abs() < 0.02 * want_ux,
            "peak u_x {ux} vs c·L/E {want_ux}"
        );
    }

    /// The case-level upper bound: with `equilibrate` set, `analyse_full` recovers the
    /// statically admissible stress field per case and carries C(σ*) ≥ the interpolated
    /// truth (certified up to the P2 interpolation defect). The pulled bar's exact
    /// solution is in the span, so the recovered bound is tight: C(σ*) ≥ compliance −
    /// solver slop, both at the closed form c²·V/E.
    #[test]
    fn case_compliance_upper_recovers_the_certified_bracket() {
        let d = parse(PULL);
        let young = materials::find(&d, "petg").expect("petg").e;
        let want = 10.0 * 10.0 * 40.0 / (young * 100.0);
        // The production-size path: the sparse iterative recovery carries the flag far
        // beyond the dense budget, so the analysis-lens mesh gets a real bracket.
        let t_rec = std::time::Instant::now();
        let a = analyse_full(&d, Resolution::Size(2.0), now, None, true).expect("analyse");
        eprintln!("EQUILIBRATION WALL (ms): {}", t_rec.elapsed().as_millis());
        let case = a
            .cases
            .iter()
            .find(|c| c.name == "Load 1")
            .expect("load case");
        assert!(case.solve.converged);
        let up = case.compliance_upper.expect("the flag was set");
        // In-span: both ends at the closed form; the recovery's witness set held or the
        // recovery would have refused the whole analysis.
        assert!(
            (case.compliance - want).abs() < 0.02 * want,
            "lower end want={want}"
        );
        assert!(
            (up - want).abs() < 0.02 * want,
            "upper end: {up} vs want {want}"
        );
        assert!(up >= case.compliance - 1e-9, "bracket ordering");
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
        // Every mode carries a shape, and the shapes differ (M-ordering is not degenerate
        // here: a repeated frequency would come back as two distinct vectors anyway).
        let shapes = &run.modal.shapes;
        assert_eq!(shapes.len(), fs.len(), "one shape per frequency");
        for (i, s) in shapes.iter().enumerate() {
            assert!(!s.is_empty(), "shape {i} empty");
            assert!(s.iter().any(|v| v.abs() > 1e-9), "shape {i} is flat");
        }
        assert_ne!(
            shapes[0], shapes[1],
            "the first two shapes would animate identically"
        );
        // The static path through the same document still wants a load.
        let err = analyse_full(&d, Resolution::Size(1.5), now, None, false).expect_err("no loads");
        assert_eq!(err.0, "add a load");
        // And with a load, the same document carries the modes on the shared mesh.
        let doc = parse(OVERLAP);
        let a = analyse_full(&doc, Resolution::Size(3.0), now, Some(2), false).expect("analyse");
        let modes = a.modes.as_ref().expect("modes");
        assert_eq!(modes.frequencies.len(), 2);
        assert!(modes.frequencies.iter().all(|f| *f > 0.0));
        assert_eq!(modes.shapes.len(), 2, "one shape per requested mode");
        // provenance stays the static pipeline's: mesh + static solves (no modal time).
        assert!(
            (a.provenance.total_ms - a.provenance.mesh_ms - a.provenance.solve_ms).abs() < 50.0,
            "total {} ≠ mesh {} + solve {}",
            a.provenance.total_ms,
            a.provenance.mesh_ms,
            a.provenance.solve_ms
        );
    }

    /// Thermal flow through the pipeline (M14): a heated cantilever drives a "Steady"
    /// case beside the mechanical one — nodal temperature carries the stated face
    /// values at the region's nodes and the conduction report arrives; the mechanical
    /// cases carry neither; thermal cases run after mechanical ones; provenance counts
    /// the thermal leg.
    const THERMAL: &str = "\
name \"Heated bar\"
custommaterial alu e=70000 nu=0.33 a=2.3e-5 yield=250 density=2.7
material alu
box bar size=(40, 10, 10) at=(20, 5, 5)
region cold = bar.px
region hot = bar.nx
support clamp cold fixed=xyz
temperature root cold temp=20 case=\"Steady\"
temperature tip hot temp=90 case=\"Steady\"
load pull hot force=(10, 0, 0)
";

    #[test]
    fn thermal_case_runs_beside_the_mechanical_cases() {
        let d = parse(THERMAL);
        let a = analyse(&d, Resolution::Size(2.0), now).expect("analyse");
        let mech = a
            .cases
            .iter()
            .find(|c| c.name == "Load 1")
            .expect("load case");
        assert!(mech.temperature.is_none() && mech.conduction.is_none());
        let th = a
            .cases
            .iter()
            .find(|c| c.name == "Steady")
            .expect("thermal case");
        let th_ix = a.cases.iter().position(|c| c.name == "Steady").unwrap();
        assert!(
            th_ix > a.cases.iter().position(|c| c.name == "Load 1").unwrap(),
            "thermal cases run mechanical-first"
        );
        // Dirichlet faces: the fixed-end and hot-end nodes hold the stated values.
        let t = th.temperature.as_ref().expect("temperature field");
        let (mut cold, mut hot) = (None, None);
        for (n, p) in a.mesh.nodes.iter().enumerate() {
            if p[0] > 40.0 - 1e-9 {
                cold = Some((n, t[n]));
            } else if p[0] < 1e-9 {
                hot = Some((n, t[n]));
            }
        }
        let (_, tc) = cold.expect("cold-face node");
        let (_, thv) = hot.expect("hot-face node");
        assert!((tc - 20.0).abs() < 1e-9, "cold face {} °C", tc);
        assert!((thv - 90.0).abs() < 1e-9, "hot face {} °C", thv);
        // Laplace with linear face data: every interior node sits on the linear ramp
        // (hot at x=0, cold at x=40).
        for (tn, p) in a.mesh.nodes.iter().enumerate() {
            if p[0] > 1e-9 && p[0] < 40.0 - 1e-9 {
                let want = 90.0 - 70.0 * p[0] / 40.0;
                assert!(
                    (t[tn] - want).abs() < 0.5,
                    "node at {:.3} holds {} °C, linear ramp says {:.3}",
                    p[0],
                    t[tn],
                    want
                );
            }
        }
        let cr = th.conduction.as_ref().expect("conduction report");
        assert!(cr.converged && cr.iterations > 0);
        // The expansion solved for something measurable.
        assert!(
            th.max_displacement > 0.0,
            "thermal expansion displaced nothing"
        );
        // The thermal pseudo-load's own compliance: a(u,u) ≥ 0 for this converged solve.
        assert!(th.compliance > 0.0, "thermal compliance");
        // Provenance: the thermal leg is timed beside the static one.
        assert!(
            a.provenance.thermal_ms > 0.0
                && (a.provenance.total_ms
                    - a.provenance.mesh_ms
                    - a.provenance.solve_ms
                    - a.provenance.thermal_ms)
                    .abs()
                    < 50.0,
            "total {} ≠ mesh {} + solve {} + thermal {}",
            a.provenance.total_ms,
            a.provenance.mesh_ms,
            a.provenance.solve_ms,
            a.provenance.thermal_ms
        );
    }

    /// Two temperature statements in one case meeting at the shared edge of px and py:
    /// the shared nodes cannot carry both values — named, not silently merged.
    const TEMPERATURE_CLASH: &str = "\
name \"Clashing temperatures\"
custommaterial alu e=70000 nu=0.33 a=2.3e-5 yield=250 density=2.7
material alu
box bar size=(40, 10, 10) at=(20, 5, 5)
region right = bar.px
region top = bar.py
region root_face = bar.nx
support clamp root_face fixed=xyz
temperature cold right temp=20
temperature warm top temp=80
";

    #[test]
    fn temperature_clash_at_shared_edge_is_named() {
        let d = parse(TEMPERATURE_CLASH);
        let err = analyse(&d, Resolution::Size(2.0), now).expect_err("clash");
        assert!(err.0.contains("meet at shared surface nodes"), "{}", err.0);
        assert!(
            err.0.contains("'cold'") && err.0.contains("'warm'"),
            "{}",
            err.0
        );
    }

    /// A temperature region that never reaches the surface is named like a support's
    /// and a load's: a sphere buried inside the bar selects no boundary facet.
    const TEMPERATURE_ABSENT: &str = "\
name \"Absent temperature\"
custommaterial alu e=70000 nu=0.33 a=2.3e-5 yield=250 density=2.7
material alu
group g {
  box bar size=(40, 10, 10) at=(20, 5, 5)
  sphere inner r=2 at=(20, 5, 5)
}
region root_face = bar.nx
region ghost = inner.all
support clamp root_face fixed=xyz
temperature warm ghost temp=80
";

    #[test]
    fn temperature_off_the_surface_is_named() {
        let d = parse(TEMPERATURE_ABSENT);
        let err = analyse(&d, Resolution::Size(2.0), now).expect_err("no-touch");
        assert_eq!(
            err.0,
            "temperature 'warm': region 'ghost' does not touch the part's surface"
        );
    }

    /// Convection through the pipeline (§7 item 3): a fixed hot face at x=0 and an air
    /// band on x=40. The exact 1-D wall: flux q = (100−20)/(L/k + 1/h) = 80/50 = 1.6 with
    /// k = 1 (the solver's pin), so T = 100 − 1.6·x and the surface sits at 36 °C. The
    /// linear profile satisfies every discrete equation pointwise (the solve-level bar
    /// test proves the mechanism), so the pipeline must reproduce it to PCG accuracy.
    const CONVECTION: &str = "\
name \"Cooled bar\"
custommaterial alu e=70000 nu=0.33 a=2.3e-5 yield=250 density=2.7
material alu
box bar size=(40, 10, 10) at=(20, 5, 5)
region root_face = bar.nx
region air = bar.px
support clamp root_face fixed=xyz
temperature root root_face temp=100 case=\"Steady\"
convection air_band air h=0.1 tinf=20 case=\"Steady\"
";

    #[test]
    fn convection_band_drives_the_thermal_case() {
        let d = parse(CONVECTION);
        let a = analyse(&d, Resolution::Size(2.0), now).expect("analyse");
        let th = a
            .cases
            .iter()
            .find(|c| c.name == "Steady")
            .expect("thermal case");
        let t = th.temperature.as_ref().expect("temperature field");
        for (n, p) in a.mesh.nodes.iter().enumerate() {
            let want = 100.0 - 1.6 * p[0];
            assert!(
                (t[n] - want).abs() < 1e-6,
                "node at ({:.3}, {:.3}, {:.3}): {} °C, analytic wall says {:.4}",
                p[0],
                p[1],
                p[2],
                t[n],
                want
            );
        }
        // Free lateral faces stay insulated; the band's facets alone set the flux.
        let cr = th.conduction.as_ref().expect("conduction report");
        assert!(cr.converged && cr.iterations > 0);
        // The gradient field still drives measurable expansion.
        assert!(
            th.max_displacement > 0.0,
            "thermal expansion displaced nothing"
        );
    }

    /// The flux twin of CONVECTION: a prescribed inflow of q = 2 W/mm² on the far face
    /// drives T = 100 + 2x through the same bar, so the driven face sits at 180 °C.
    const HEATFLUX: &str = "\
name \"Heated bar\"
custommaterial alu e=70000 nu=0.33 a=2.3e-5 yield=250 density=2.7
material alu
box bar size=(40, 10, 10) at=(20, 5, 5)
region root_face = bar.nx
region hot = bar.px
support clamp root_face fixed=xyz
temperature root root_face temp=100 case=\"Steady\"
heatflux inflow hot q=2 case=\"Steady\"
";

    #[test]
    fn heatflux_band_drives_the_thermal_case() {
        let d = parse(HEATFLUX);
        let a = analyse(&d, Resolution::Size(2.0), now).expect("analyse");
        let th = a
            .cases
            .iter()
            .find(|c| c.name == "Steady")
            .expect("thermal case");
        let t = th.temperature.as_ref().expect("temperature field");
        for (n, p) in a.mesh.nodes.iter().enumerate() {
            let want = 100.0 + 2.0 * p[0];
            assert!(
                (t[n] - want).abs() < 1e-6,
                "node at ({:.3}, {:.3}, {:.3}): {} °C, analytic wall says {:.4}",
                p[0],
                p[1],
                p[2],
                t[n],
                want
            );
        }
        let cr = th.conduction.as_ref().expect("conduction report");
        assert!(cr.converged && cr.iterations > 0);
        assert!(
            th.max_displacement > 0.0,
            "thermal expansion displaced nothing"
        );
    }

    /// A temperature and a convection may name the same region and case: the fixed value
    /// wins (its lift runs over the band's assembly, and all the band's nodes lie on the
    /// fixed face), so T ≡ 80 — the hot-wall-washed-by-coolant shape.
    const CONVECTION_CLAMPED: &str = "\
name \"Washed wall\"
custommaterial alu e=70000 nu=0.33 a=2.3e-5 yield=250 density=2.7
material alu
box bar size=(40, 10, 10) at=(20, 5, 5)
region root_face = bar.nx
region wash = bar.px
support clamp root_face fixed=xyz
temperature fixed_root wash temp=80 case=\"Steady\"
convection wash_band wash h=0.1 tinf=20 case=\"Steady\"
";

    #[test]
    fn temperature_wins_where_it_overlaps_a_convection() {
        let d = parse(CONVECTION_CLAMPED);
        let a = analyse(&d, Resolution::Size(2.0), now).expect("analyse");
        let th = a
            .cases
            .iter()
            .find(|c| c.name == "Steady")
            .expect("thermal case");
        let t = th.temperature.as_ref().expect("temperature field");
        let max_dev = t
            .iter()
            .zip(&a.mesh.nodes)
            .map(|(&tn, _)| (tn - 80.0).abs())
            .fold(0.0, f64::max);
        assert!(
            max_dev < 1e-6,
            "the fixed value must win on the band's own nodes; worst deviation {max_dev}"
        );
    }

    /// A convection whose region never reaches the surface is named like a temperature's.
    const CONVECTION_ABSENT: &str = "\
name \"Absent convection\"
custommaterial alu e=70000 nu=0.33 a=2.3e-5 yield=250 density=2.7
material alu
group g {
  box bar size=(40, 10, 10) at=(20, 5, 5)
  sphere inner r=2 at=(20, 5, 5)
}
region root_face = bar.nx
region ghost = inner.all
support clamp root_face fixed=xyz
convection bath ghost h=1e-5 tinf=50
";

    #[test]
    fn convection_off_the_surface_is_named() {
        let d = parse(CONVECTION_ABSENT);
        let err = analyse(&d, Resolution::Size(2.0), now).expect_err("no-touch");
        assert_eq!(
            err.0,
            "convection 'bath': region 'ghost' does not touch the part's surface"
        );
    }
}
