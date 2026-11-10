//! The analysis pipeline: document → field → tetrahedral mesh → boundary conditions → P2 solve →
//! nodal results. Runs identically natively (CLI, verification tests) and in a Web Worker.

pub mod fill;
pub mod fsmz;
pub mod import;
pub mod overhang;
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
    compliance, element_stress, element_stress_thermal, layer_peel_utilization, min_compressive,
    nodal_stress, nodal_stress_mat, nodal_stress_thermal, nodal_stress_thermal_multi, solve,
    solve_conduction, thermal_load, thermal_load_multi, upper_compliance, von_mises, HeatFluxBc,
    P2Mesh, Problem, RobinBc, SolveReport, SolverOptions, Stress, ThermalRefData, V3,
};
use serde::Serialize;

pub use fieldsmith_solve::Material;
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
    /// Most compressive principal stress at each element centroid (≤ 0; MPa) — the
    /// buckling indicator's load measure, the twin of `vm_elem`. Serde-skipped like the
    /// other per-element fields.
    #[serde(skip)]
    pub sigma_min_elem: Vec<f64>,
    /// Element-centroid stress tensors `σ` (MPa) — kept in memory for layer-peel and
    /// build-orientation evaluation.
    #[serde(skip)]
    pub stress_elem: Vec<Stress>,
    /// Equivalent plastic strain at each element centroid (`%`, `0.0` on elastic/linear cases).
    #[serde(skip)]
    pub plastic_strain_elem: Vec<f64>,
    /// Nodal 3D temperature gradient `[∂T/∂x, ∂T/∂y, ∂T/∂z]` (`°C/mm`, `3 * nn` floats) on thermal cases.
    #[serde(skip)]
    pub grad_t_nodal: Option<Vec<f64>>,
    /// Nodal inter-layer peel utilization `√((max(0, σ_nn)/S_z)² + (τ_nt/S_s)²)` (dimensionless).
    #[serde(skip)]
    pub peel_nodal: Vec<f64>,
    /// Element-centroid inter-layer peel utilization (dimensionless).
    #[serde(skip)]
    pub peel_elem: Vec<f64>,
    /// Peak element-centroid inter-layer peel utilization across this case.
    #[serde(skip)]
    pub max_layer_peel: f64,
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
    pub mass_g: f64,
    pub yield_strength: f64,
    pub yield_z: f64,
    pub shear_z: f64,
    pub build_angles: [f64; 2],
    pub best_build_angles: [f64; 2],
    pub min_peak_peel: f64,
    pub worst_build_angles: [f64; 2],
    pub max_peak_peel: f64,
    pub cases: Vec<CaseResult>,
    /// Max over cases of nodal von Mises.
    pub envelope: Vec<f64>,
    /// Max over cases of element-centroid von Mises.
    pub envelope_elem: Vec<f64>,
    /// Max over cases of element-centroid inter-layer peel utilization.
    pub peel_envelope_elem: Vec<f64>,
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

/// The most compressive principal stress at each element centroid (0 when the tensor has
/// no compressive component) — the element buckling indicator reads. Twin of
/// [`elem_von_mises`] with the same centroid statistic.
pub fn elem_min_compress_principal(
    mesh: &P2Mesh,
    u: &[f64],
    young: &[f64],
    poisson: f64,
) -> Vec<f64> {
    (0..mesh.tets.len())
        .map(|e| min_compressive(&element_stress(mesh, u, e, young[e], poisson, [0.25; 3])))
        .collect()
}

/// [`elem_von_mises`] routed by material: cubic elements read the cubic kernel's tensor.
pub fn elem_von_mises_mat(mesh: &P2Mesh, u: &[f64], mat: &Material) -> Vec<f64> {
    (0..mesh.tets.len())
        .map(|e| {
            von_mises(&fieldsmith_solve::element_stress_mat(
                mesh, u, e, mat, [0.25; 3],
            ))
        })
        .collect()
}

/// [`elem_min_compress_principal`] routed by material.
pub fn elem_min_compress_principal_mat(mesh: &P2Mesh, u: &[f64], mat: &Material) -> Vec<f64> {
    (0..mesh.tets.len())
        .map(|e| {
            min_compressive(&fieldsmith_solve::element_stress_mat(
                mesh, u, e, mat, [0.25; 3],
            ))
        })
        .collect()
}

/// [`elem_min_compress_principal`] for a thermal case: the elastic-thermal tensor's
/// most compressive principal (same thermal term [`elem_von_mises_thermal`] subtracts).
pub fn elem_min_compress_principal_thermal(
    mesh: &P2Mesh,
    u: &[f64],
    young: &[f64],
    poisson: f64,
    alpha: f64,
    dt: &[f64],
) -> Vec<f64> {
    use fenics_element::{FiniteElement, P2Tetrahedron};
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
            min_compressive(&s)
        })
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
    elem_stress_thermal(mesh, u, young, poisson, alpha, dt)
        .iter()
        .map(von_mises)
        .collect()
}

/// Element-centroid stress tensor for each element (`xi = [0.25; 3]`), isotropic.
pub fn elem_stress(mesh: &P2Mesh, u: &[f64], young: &[f64], poisson: f64) -> Vec<Stress> {
    (0..mesh.tets.len())
        .map(|e| element_stress(mesh, u, e, young[e], poisson, [0.25; 3]))
        .collect()
}

/// Element-centroid stress tensor routed by material (cubic elements use the cubic kernel).
pub fn elem_stress_mat(mesh: &P2Mesh, u: &[f64], mat: &Material) -> Vec<Stress> {
    (0..mesh.tets.len())
        .map(|e| fieldsmith_solve::element_stress_mat(mesh, u, e, mat, [0.25; 3]))
        .collect()
}

/// Element-centroid stress tensor for a thermal case (`element_stress_thermal` at `[0.25; 3]`).
pub fn elem_stress_thermal(
    mesh: &P2Mesh,
    u: &[f64],
    young: &[f64],
    poisson: f64,
    alpha: f64,
    dt: &[f64],
) -> Vec<Stress> {
    use fenics_element::{FiniteElement, P2Tetrahedron};
    let phi = P2Tetrahedron.evaluate_basis(&[0.25, 0.25, 0.25]);
    (0..mesh.tets.len())
        .map(|e| {
            let t = &mesh.tets[e];
            let dt_c: f64 = t
                .iter()
                .zip(phi.iter())
                .map(|(&n, p)| dt[n as usize] * p)
                .sum();
            element_stress_thermal(mesh, u, e, young[e], poisson, alpha, dt_c, [0.25; 3])
        })
        .collect()
}

/// Multi-material [`elem_stress_thermal`]: one thermal expansion coefficient
/// `alpha_elem[e]` per element (V53).
pub fn elem_stress_thermal_multi(
    mesh: &P2Mesh,
    u: &[f64],
    young: &[f64],
    poisson: f64,
    alpha_elem: &[f64],
    dt: &[f64],
) -> Vec<Stress> {
    use fenics_element::{FiniteElement, P2Tetrahedron};
    let phi = P2Tetrahedron.evaluate_basis(&[0.25, 0.25, 0.25]);
    (0..mesh.tets.len())
        .map(|e| {
            let t = &mesh.tets[e];
            let dt_c: f64 = t
                .iter()
                .zip(phi.iter())
                .map(|(&n, p)| dt[n as usize] * p)
                .sum();
            element_stress_thermal(
                mesh,
                u,
                e,
                young[e],
                poisson,
                alpha_elem[e],
                dt_c,
                [0.25; 3],
            )
        })
        .collect()
}

/// Populate `peel_nodal`, `peel_elem`, and `max_layer_peel` on a solved `CaseResult`
/// for build axis `b_hat` and inter-layer strengths `(yield_z, shear_z)`.
pub fn populate_case_peel(case: &mut CaseResult, b_hat: [f64; 3], yield_z: f64, shear_z: f64) {
    case.peel_nodal = case
        .stress
        .iter()
        .map(|s| layer_peel_utilization(s, b_hat, yield_z, shear_z))
        .collect();
    case.peel_elem = case
        .stress_elem
        .iter()
        .map(|s| layer_peel_utilization(s, b_hat, yield_z, shear_z))
        .collect();
    case.max_layer_peel = case.peel_elem.iter().copied().fold(0.0, f64::max);
}

/// Sweep build orientations `(theta_deg, phi_deg)` over the upper hemisphere
/// (`theta ∈ {0°, 15°, …, 90°}`, `phi ∈ {0°, 30°, …, 330°}`) across `cases` to find the
/// build orientation that minimizes and maximizes peak element-centroid layer-peel
/// utilization (`(best_angles, min_peak, worst_angles, max_peak)`).
pub fn sweep_build_orientations(
    cases: &[CaseResult],
    yield_z: f64,
    shear_z: f64,
    class: Option<&[u8]>,
) -> ([f64; 2], f64, [f64; 2], f64) {
    if cases.is_empty() || yield_z <= 0.0 || shear_z <= 0.0 {
        return ([0.0, 0.0], 0.0, [0.0, 0.0], 0.0);
    }
    let mut best_ang = [0.0, 0.0];
    let mut best_val = f64::INFINITY;
    let mut worst_ang = [0.0, 0.0];
    let mut worst_val = 0.0_f64;
    for ti in 0..=6 {
        let theta = (ti as f64) * 15.0;
        let phi_steps = if ti == 0 { 1 } else { 12 };
        for pi in 0..phi_steps {
            let phi = (pi as f64) * 30.0;
            let b_hat = crate::overhang::build_axis_from_angles(theta, phi);
            let mut peak = 0.0_f64;
            for c in cases {
                for (e, s) in c.stress_elem.iter().enumerate() {
                    if let Some(cls) = class {
                        if cls.get(e).copied() == Some(crate::fill::CLASS_VOID) {
                            continue;
                        }
                    }
                    let u = layer_peel_utilization(s, b_hat, yield_z, shear_z);
                    if u > peak {
                        peak = u;
                    }
                }
            }
            if peak < best_val {
                best_val = peak;
                best_ang = [theta, phi];
            }
            if peak > worst_val || worst_val == 0.0 {
                worst_val = peak;
                worst_ang = [theta, phi];
            }
        }
    }
    if !best_val.is_finite() {
        best_val = 0.0;
    }
    (best_ang, best_val, worst_ang, worst_val)
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
    // In multi-body assemblies or coarse voxelizations where a sub-volume is either
    // completely disconnected or attached only by a 1-vertex point hinge or 1-edge pin
    // hinge (sharing no 3-vertex triangular face with the supported structure), group
    // tetrahedra into face-connected 3D solid components and pin any component that has
    // no supported face. This eliminates both floating rigid-body modes and edge/vertex
    // hinge mechanisms while leaving every face-connected supported solid untouched.
    if fixed.iter().any(|&m| m != 0) && !mesh.tets.is_empty() {
        let nt = mesh.tets.len();
        let mut parent: Vec<usize> = (0..nt).collect();
        fn find(parent: &mut [usize], mut x: usize) -> usize {
            while parent[x] != x {
                let p = parent[x];
                parent[x] = parent[p];
                x = p;
            }
            x
        }
        let mut face_owner: std::collections::HashMap<[u32; 3], usize> =
            std::collections::HashMap::with_capacity(nt * 2);
        const TET_FACES: [[usize; 3]; 4] = [[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]];
        for (ei, tet) in mesh.tets.iter().enumerate() {
            for f in TET_FACES {
                let mut key = [tet[f[0]], tet[f[1]], tet[f[2]]];
                key.sort_unstable();
                if let Some(&other) = face_owner.get(&key) {
                    let r1 = find(&mut parent, ei);
                    let r2 = find(&mut parent, other);
                    if r1 != r2 {
                        parent[r2] = r1;
                    }
                } else {
                    face_owner.insert(key, ei);
                }
            }
        }
        let mut comp_supported = vec![false; nt];
        for (ei, tet) in mesh.tets.iter().enumerate() {
            let has_supported_face = TET_FACES
                .iter()
                .any(|f| f.iter().all(|& j| fixed[tet[j] as usize] != 0));
            if has_supported_face {
                let r = find(&mut parent, ei);
                comp_supported[r] = true;
            }
        }
        if comp_supported.iter().any(|&s| s) {
            for (ei, tet) in mesh.tets.iter().enumerate() {
                let r = find(&mut parent, ei);
                if !comp_supported[r] {
                    for &v in tet {
                        fixed[v as usize] = 0b111;
                    }
                }
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
                let lets: Vec<(String, f64)> =
                    doc.lets.iter().map(|l| (l.name.clone(), l.value)).collect();
                let f = LoadFn::compile_with_lets(expr, &lets)
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
        // Pre-compute split-axis preload geometry when `axis != [0, 0, 0]`.
        let preload_split: Option<([f64; 3], [f64; 3], f64, f64)> = match l.kind {
            LoadKind::Preload { axis, .. } if axis != [0.0; 3] => {
                let len = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
                if len == 0.0 {
                    return Err(SetupError(format!(
                        "load '{}': preload axis is the zero vector",
                        l.id
                    )));
                }
                let ahat = axis.map(|c| c / len);
                let mut c_mean = [0.0f64; 3];
                for &fi in &fs {
                    let fc = facets[fi].centroid(mesh);
                    let fa = facets[fi].area(mesh);
                    for k in 0..3 {
                        c_mean[k] += fa * fc[k];
                    }
                }
                for c in &mut c_mean {
                    *c /= area;
                }
                let mut a_pos = 0.0f64;
                let mut a_neg = 0.0f64;
                for &fi in &fs {
                    let fc = facets[fi].centroid(mesh);
                    let fa = facets[fi].area(mesh);
                    let proj = (fc[0] - c_mean[0]) * ahat[0]
                        + (fc[1] - c_mean[1]) * ahat[1]
                        + (fc[2] - c_mean[2]) * ahat[2];
                    if proj >= 0.0 {
                        a_pos += fa;
                    } else {
                        a_neg += fa;
                    }
                }
                Some((ahat, c_mean, a_pos, a_neg))
            }
            _ => None,
        };
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
                    LoadKind::Preload { newtons, .. } => match preload_split {
                        Some((ahat, c_mean, a_pos, a_neg))
                            if a_pos > 1e-6 * area && a_neg > 1e-6 * area =>
                        {
                            let fc = fct.centroid(mesh);
                            let proj = (fc[0] - c_mean[0]) * ahat[0]
                                + (fc[1] - c_mean[1]) * ahat[1]
                                + (fc[2] - c_mean[2]) * ahat[2];
                            if proj >= 0.0 {
                                ahat.map(|c| -(newtons / a_pos) * c)
                            } else {
                                ahat.map(|c| (newtons / a_neg) * c)
                            }
                        }
                        Some((ahat, _, _, _)) => ahat.map(|c| -(newtons / area) * c),
                        None => fct.normal(mesh).map(|c| -(newtons / area) * c),
                    },
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
                let mag = match l.kind {
                    LoadKind::Preload { newtons, .. } => newtons.abs(),
                    _ => (resultant[0].powi(2) + resultant[1].powi(2) + resultant[2].powi(2)).sqrt(),
                };
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
    material: &Material,
    fixed: &[u8],
    force: Vec<f64>,
    name: String,
    equilibrate: bool,
    prescribed: &dyn Fn([V3; 3]) -> Option<V3>,
) -> Result<CaseResult, SetupError> {
    let problem = Problem {
        mesh: mesh.clone(),
        material: material.clone(),
        fixed: fixed.to_vec(),
        force,
    };
    // The equilibrated upper end reads each element's compliance tensor — isotropic
    // Gram or cubic Gram (V43) — so the cubic route certifies through the same law the
    // conforming solve used.
    let (u, rep) = solve(&problem, SolverOptions::default()).map_err(SetupError)?;
    let routed = problem.material.cubic.is_some();
    let (stress, stress_elem) = if routed {
        (
            nodal_stress_mat(mesh, &u, &problem.material),
            elem_stress_mat(mesh, &u, &problem.material),
        )
    } else {
        (
            nodal_stress(mesh, &u, &problem.material.young, problem.material.poisson),
            elem_stress(mesh, &u, &problem.material.young, problem.material.poisson),
        )
    };
    let vm_elem: Vec<f64> = stress_elem.iter().map(von_mises).collect();
    let sigma_min_elem: Vec<f64> = stress_elem.iter().map(min_compressive).collect();
    let vm: Vec<f64> = stress.iter().map(von_mises).collect();
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
        sigma_min_elem,
        stress_elem,
        plastic_strain_elem: vec![0.0; mesh.tets.len()],
        grad_t_nodal: None,
        peel_nodal: Vec::new(),
        peel_elem: vec![0.0; mesh.tets.len()],
        max_layer_peel: 0.0,
        vm_elem,
        solve: rep,
        load_reports: Vec::new(),
        temperature: None,
        conduction: None,
    })
}

/// 3D Nonlinear Elasto-Plastic ($J_2$ Deformation / Ramberg–Osgood–Hollomon Secant-Modulus)
/// case solver: performs a linear Hookean trial solve first, and if any element exceeds
/// `yield_strength` ($\sigma_{\text{vM},e} > \sigma_y$), runs damped secant-modulus iterations
/// updating each yielded element's effective Young's modulus
/// $E_{\text{sec},e} = \sigma_{\text{flow}}(\varepsilon_{\text{eq},e}) / \varepsilon_{\text{eq},e}$
/// along the power-law hardening curve $\sigma_{\text{flow}} = \sigma_y (\varepsilon_{\text{eq}} / \varepsilon_y)^n$.
#[allow(clippy::too_many_arguments)]
pub fn run_case_elastoplastic(
    mesh: &P2Mesh,
    young: &[f64],
    poisson: f64,
    yield_strength: f64,
    yield_elem: &[f64],
    hardening_n: f64,
    fixed: &[u8],
    force: Vec<f64>,
    name: String,
    equilibrate: bool,
    prescribed: &dyn Fn([V3; 3]) -> Option<V3>,
) -> Result<CaseResult, SetupError> {
    let base_mat = Material::iso(young.to_vec(), poisson);
    let mut cur_case = run_case(
        mesh,
        &base_mat,
        fixed,
        force.clone(),
        name.clone(),
        equilibrate,
        prescribed,
    )?;
    let any_yielded = (0..mesh.tets.len()).any(|e| {
        let sy_e = yield_elem.get(e).copied().unwrap_or(yield_strength);
        sy_e > 0.0 && cur_case.vm_elem[e] > sy_e
    });
    if !any_yielded {
        return Ok(cur_case);
    }
    let n_hard = hardening_n.clamp(0.02, 0.60);
    let mut e_sec = young.to_vec();
    let mut plastic_strain_elem = vec![0.0f64; mesh.tets.len()];
    let mut total_iters = cur_case.solve.iterations;
    let n_passes = 3usize;
    for pass in 0..n_passes {
        let mut any_updated = false;
        for (e, &e0) in young.iter().enumerate() {
            let vm = cur_case.vm_elem[e];
            let sy_e = yield_elem.get(e).copied().unwrap_or(yield_strength);
            if sy_e > 0.0 && vm > sy_e && e0 > 0.0 {
                let eps_y = (sy_e / e0).max(1e-12);
                // Neuber energy-consistent local strain estimate from current secant state:
                // sigma * eps = vm^2 / e_sec => eps_eq / eps_y = (vm / sigma_y)^(2 / (n + 1))
                let over = (vm / sy_e).max(1.0);
                let eps_ratio =
                    ((e0 / e_sec[e].max(0.02 * e0)) * over.powf(2.0 / (n_hard + 1.0))).max(1.0);
                let eps_eq = eps_y * eps_ratio;
                let sigma_flow = sy_e * eps_ratio.powf(n_hard);
                let e_target = (sigma_flow / eps_eq).clamp(0.03 * e0, e0);
                e_sec[e] = 0.25 * e_sec[e] + 0.75 * e_target;
                plastic_strain_elem[e] = ((eps_eq - sigma_flow / e0).max(0.0)) * 100.0;
                any_updated = true;
            }
        }
        if !any_updated {
            break;
        }
        let sec_mat = Material::iso(e_sec.clone(), poisson);
        let next_case = run_case(
            mesh,
            &sec_mat,
            fixed,
            force.clone(),
            name.clone(),
            equilibrate && pass + 1 == n_passes,
            prescribed,
        )?;
        total_iters += next_case.solve.iterations;
        cur_case = next_case;
    }
    cur_case.plastic_strain_elem = plastic_strain_elem;
    cur_case.solve.iterations = total_iters;
    Ok(cur_case)
}

/// Recover 3D nodal temperature gradients `[∂T/∂x, ∂T/∂y, ∂T/∂z]` (`°C/mm`, `3 * nn` floats)
/// by evaluating the exact quadratic P2 basis gradient at each of the 10 nodes of every
/// tetrahedron and volume-averaging onto the mesh nodes.
pub fn compute_nodal_temperature_gradient(mesh: &P2Mesh, t: &[f64]) -> Vec<f64> {
    use fenics_element::{FiniteElement, P2Tetrahedron};
    let nn = mesh.nodes.len();
    let pts = P2Tetrahedron.nodal_coordinates();
    let ref_grads: Vec<Vec<[f64; 3]>> = pts
        .iter()
        .map(|xi| P2Tetrahedron.evaluate_gradients(xi))
        .collect();
    let mut acc = vec![0.0f64; 3 * nn];
    let mut w = vec![0.0f64; nn];
    for (e, tet) in mesh.tets.iter().enumerate() {
        let vol = mesh.tet_volume(e).abs();
        let map = fieldsmith_solve::assemble::element_map(mesh, e);
        for (a, g_ref) in ref_grads.iter().enumerate() {
            let mut gx = 0.0f64;
            let mut gy = 0.0f64;
            let mut gz = 0.0f64;
            for b in 0..10 {
                let gp = map.transform_gradient(&g_ref[b]);
                let tb = t[tet[b] as usize];
                gx += tb * gp[0];
                gy += tb * gp[1];
                gz += tb * gp[2];
            }
            let n = tet[a] as usize;
            acc[3 * n] += vol * gx;
            acc[3 * n + 1] += vol * gy;
            acc[3 * n + 2] += vol * gz;
            w[n] += vol;
        }
    }
    for n in 0..nn {
        let wn = w[n];
        if wn > 0.0 {
            acc[3 * n] /= wn;
            acc[3 * n + 1] /= wn;
            acc[3 * n + 2] /= wn;
        }
    }
    acc
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
    let grad_t = compute_nodal_temperature_gradient(mesh, &t);
    // ε_th = α·(T − t_ref): the reference temperature is the stress-free one.
    let t_ref = doc.solve.t_ref.unwrap_or(20.0);
    let dt: Vec<f64> = t.iter().map(|ti| ti - t_ref).collect();
    let force = thermal_load(mesh, &dt, young, poisson, alpha, &ThermalRefData::new());
    let problem = Problem {
        mesh: mesh.clone(),
        material: Material {
            young: young.to_vec(),
            poisson,
            cubic: None,
        },
        fixed: fixed.to_vec(),
        force,
    };
    let (u, rep) = solve(&problem, SolverOptions::default()).map_err(SetupError)?;
    let stress = nodal_stress_thermal(mesh, &u, young, poisson, alpha, &dt);
    let stress_elem = elem_stress_thermal(mesh, &u, young, poisson, alpha, &dt);
    let vm: Vec<f64> = stress.iter().map(von_mises).collect();
    let vm_elem: Vec<f64> = stress_elem.iter().map(von_mises).collect();
    let sigma_min_elem: Vec<f64> = stress_elem.iter().map(min_compressive).collect();
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
        sigma_min_elem,
        stress_elem,
        plastic_strain_elem: vec![0.0; mesh.tets.len()],
        grad_t_nodal: Some(grad_t),
        peel_nodal: Vec::new(),
        peel_elem: vec![0.0; mesh.tets.len()],
        max_layer_peel: 0.0,
        vm_elem,
        solve: rep,
        load_reports: Vec::new(),
        temperature: Some(t),
        conduction: Some(creport),
    })
}

/// Multi-material thermal case solve (`V53`): uses per-element thermal expansion
/// `alpha_elem[e]` alongside per-element `young[e]`. Bit-identical to [`run_thermal_case`]
/// when `alpha_elem` is uniform.
#[allow(clippy::too_many_arguments)]
pub fn run_thermal_case_multi(
    doc: &Document,
    mesh: &P2Mesh,
    young: &[f64],
    poisson: f64,
    alpha_elem: &[f64],
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
    let grad_t = compute_nodal_temperature_gradient(mesh, &t);
    // Pure thermal conduction (no mechanical supports or zero CTE): return exact nodal
    // temperature field and conduction report without forcing an elastic stress solve.
    if fixed.iter().all(|&m| m == 0) || alpha_elem.iter().all(|&a| a.abs() < 1e-18) {
        let nn = mesh.nodes.len();
        let ne = mesh.tets.len();
        return Ok(CaseResult {
            name,
            max_von_mises: 0.0,
            max_vm_elem: 0.0,
            max_displacement: 0.0,
            compliance: 0.0,
            compliance_upper: if equilibrate { Some(0.0) } else { None },
            u: vec![0.0; nn * 3],
            stress: vec![Stress::default(); nn],
            von_mises: vec![0.0; nn],
            sigma_min_elem: vec![0.0; ne],
            stress_elem: vec![Stress::default(); ne],
            plastic_strain_elem: vec![0.0; ne],
            grad_t_nodal: Some(grad_t),
            peel_nodal: Vec::new(),
            peel_elem: vec![0.0; ne],
            max_layer_peel: 0.0,
            vm_elem: vec![0.0; ne],
            solve: creport.clone(),
            load_reports: Vec::new(),
            temperature: Some(t),
            conduction: Some(creport),
        });
    }
    let t_ref = doc.solve.t_ref.unwrap_or(20.0);
    let dt: Vec<f64> = t.iter().map(|ti| ti - t_ref).collect();
    let force = thermal_load_multi(mesh, &dt, young, poisson, alpha_elem, &ThermalRefData::new());
    let problem = Problem {
        mesh: mesh.clone(),
        material: Material {
            young: young.to_vec(),
            poisson,
            cubic: None,
        },
        fixed: fixed.to_vec(),
        force,
    };
    let (u, rep) = solve(&problem, SolverOptions::default()).map_err(SetupError)?;
    let stress = nodal_stress_thermal_multi(mesh, &u, young, poisson, alpha_elem, &dt);
    let stress_elem = elem_stress_thermal_multi(mesh, &u, young, poisson, alpha_elem, &dt);
    let vm: Vec<f64> = stress.iter().map(von_mises).collect();
    let vm_elem: Vec<f64> = stress_elem.iter().map(von_mises).collect();
    let sigma_min_elem: Vec<f64> = stress_elem.iter().map(min_compressive).collect();
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
        sigma_min_elem,
        stress_elem,
        plastic_strain_elem: vec![0.0; mesh.tets.len()],
        grad_t_nodal: Some(grad_t),
        peel_nodal: Vec::new(),
        peel_elem: vec![0.0; mesh.tets.len()],
        max_layer_peel: 0.0,
        vm_elem,
        solve: rep,
        load_reports: Vec::new(),
        temperature: Some(t),
        conduction: Some(creport),
    })
}

/// Collect non-suppressed additive nodes carrying an explicit `material` label distinct
/// from `doc.material`, paired with their world-frame compiled SDF expression and
/// resolved [`materials::Material`].
fn collect_labelled_volumes(
    doc: &Document,
    nodes: &[fieldsmith_doc::Node],
    parent_xforms: &[expr::Rigid],
    out: &mut Vec<(String, expr::Expr, materials::Material)>,
) -> Result<(), SetupError> {
    for n in nodes {
        if n.suppressed {
            continue;
        }
        if n.op != fieldsmith_doc::Op::Subtract {
            if let Some(mid) = &n.material {
                if mid != &doc.material {
                    let m = materials::find(doc, mid).ok_or_else(|| {
                        SetupError(format!("unknown material '{mid}' on '{}'", n.id))
                    })?;
                    let mut e = expr::compile_node(doc, n);
                    for xf in parent_xforms.iter().rev() {
                        e = expr::Expr::Xform {
                            m: *xf,
                            e: Box::new(e),
                        };
                    }
                    out.push((n.id.clone(), e, m));
                }
            }
            if let fieldsmith_doc::NodeKind::Group { children, .. } = &n.kind {
                let mut next = parent_xforms.to_vec();
                if !n.transform.is_identity() {
                    next.push(expr::Rigid::from_doc(&n.transform));
                }
                collect_labelled_volumes(doc, children, &next, out)?;
            }
        }
    }
    Ok(())
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
    yield_elem: Vec<f64>,
    k_elem: Vec<f64>,
    alpha_elem: Vec<Option<f64>>,
    rho_avg: f64,
    mass_g: f64,
    h: f64,
    quality: Quality,
    mesh_ms: f64,
}

fn has_non_structural_overlays(nodes: &[fieldsmith_doc::Node]) -> bool {
    for n in nodes {
        if !n.suppressed
            && (n.id.starts_with("rf_3d_")
                || n.id.starts_with("lobe_")
                || n.id.starts_with("prop_disc_"))
        {
            return true;
        }
        if let fieldsmith_doc::NodeKind::Group { children, .. } = &n.kind {
            if has_non_structural_overlays(children) {
                return true;
            }
        }
    }
    false
}

fn suppress_non_structural_overlays(nodes: &mut [fieldsmith_doc::Node]) {
    for n in nodes {
        if n.id.starts_with("rf_3d_")
            || n.id.starts_with("lobe_")
            || n.id.starts_with("prop_disc_")
        {
            n.suppressed = true;
            continue;
        }
        if let fieldsmith_doc::NodeKind::Group { children, .. } = &mut n.kind {
            suppress_non_structural_overlays(children);
        }
    }
}

fn structural_doc(doc: &Document) -> std::borrow::Cow<'_, Document> {
    if !has_non_structural_overlays(&doc.root) {
        return std::borrow::Cow::Borrowed(doc);
    }
    let mut cloned = doc.clone();
    suppress_non_structural_overlays(&mut cloned.root);
    std::borrow::Cow::Owned(cloned)
}

fn tet_signed_vol(verts: &[[f64; 3]], t: [u32; 4]) -> f64 {
    let [a, b, c, d] = t.map(|i| verts[i as usize]);
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let w = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
    (u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0])
        + u[2] * (v[0] * w[1] - v[1] * w[0]))
        / 6.0
}

/// Remove lone 1-tet and 2-tet exterior "ear" spikes (`tet_count[v] <= 2` with no interior edges,
/// i.e. `bnd_deg[v] == tet_count[v] + 2`, at a boundary vertex `v` not lying on any bounding-box
/// datum plane `b_lo` / `b_hi`). On off-lattice horizontal edges (such as `flange_boss` at z = 26 mm
/// and `base_block` at z = 10 mm on T15), BCC short edges emit isolated 1-tet pyramid teeth and
/// 2-tet grazing wedges every cell spacing h; stripping these ear tets exposes the underlying
/// uniform bevel strip without altering datum corners.
fn prune_boundary_ear_tets(
    lin: &mut fieldsmith_mesh::TetMesh,
    b_lo: [f64; 3],
    b_hi: [f64; 3],
    h: f64,
) {
    let is_on_datum_plane = |p: [f64; 3]| -> bool {
        (0..3).any(|axis| {
            (p[axis] - b_lo[axis]).abs() <= 1e-3 * h || (p[axis] - b_hi[axis]).abs() <= 1e-3 * h
        })
    };
    for _pass in 0..2 {
        let nv = lin.verts.len();
        if nv == 0 || lin.tets.len() <= 8 {
            break;
        }
        let mut tet_count = vec![0usize; nv];
        for t in &lin.tets {
            for &vi in t {
                tet_count[vi as usize] += 1;
            }
        }
        let bnd = lin.boundary();
        let mut bnd_face_count = vec![0usize; nv];
        for &(f, _) in &bnd {
            bnd_face_count[f[0] as usize] += 1;
            bnd_face_count[f[1] as usize] += 1;
            bnd_face_count[f[2] as usize] += 1;
        }
        let before = lin.tets.len();
        lin.tets.retain(|t| {
            !t.iter().any(|&vi| {
                let v = vi as usize;
                let tc = tet_count[v];
                let is_ear = tc == 1 || (tc == 2 && lin.verts[v][2] < b_hi[2] - 1.9 * h);
                is_ear
                    && bnd_face_count[v] >= tc + 2
                    && !is_on_datum_plane(lin.verts[v])
            })
        });
        if lin.tets.len() == before {
            break;
        }
        lin.compact();
    }
}

/// Snap chamfered boundary vertices onto sharp CAD feature edges on bounding-box datum caps
/// (2-plane QEF + exact SDF positive-boundary bisection along the face tangent) while guarding
/// element volumes. Returns a per-vertex mask indicating which linear vertices lie on a datum-cap
/// 2-face CAD edge.
fn snap_feature_edges(
    lin: &mut fieldsmith_mesh::TetMesh,
    body: &expr::Expr,
    b_lo: [f64; 3],
    b_hi: [f64; 3],
    h: f64,
) -> Vec<bool> {
    let nv = lin.verts.len();
    if nv == 0 || lin.tets.is_empty() {
        return Vec::new();
    }
    let is_on_datum_plane = |p: [f64; 3]| -> bool {
        (0..3).any(|axis| {
            (p[axis] - b_lo[axis]).abs() <= 0.03 * h || (p[axis] - b_hi[axis]).abs() <= 0.03 * h
        })
    };
    let bnd = lin.boundary();
    let mut is_bnd = vec![false; nv];
    let mut mesh_norm = vec![[0.0f64; 3]; nv];
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); nv];
    for &(f, _) in &bnd {
        let [i0, i1, i2] = [f[0] as usize, f[1] as usize, f[2] as usize];
        is_bnd[i0] = true;
        is_bnd[i1] = true;
        is_bnd[i2] = true;
        for &(a, b) in &[(i0, i1), (i1, i2), (i2, i0)] {
            if !adj[a].contains(&b) {
                adj[a].push(b);
            }
            if !adj[b].contains(&a) {
                adj[b].push(a);
            }
        }
        let p0 = lin.verts[i0];
        let p1 = lin.verts[i1];
        let p2 = lin.verts[i2];
        let ux = p1[0] - p0[0];
        let uy = p1[1] - p0[1];
        let uz = p1[2] - p0[2];
        let vx = p2[0] - p0[0];
        let vy = p2[1] - p0[1];
        let vz = p2[2] - p0[2];
        let nx = uy * vz - uz * vy;
        let ny = uz * vx - ux * vz;
        let nz = ux * vy - uy * vx;
        for &idx in &[i0, i1, i2] {
            mesh_norm[idx][0] += nx;
            mesh_norm[idx][1] += ny;
            mesh_norm[idx][2] += nz;
        }
    }
    for v in 0..nv {
        if !is_bnd[v] {
            continue;
        }
        let nl = (mesh_norm[v][0].powi(2) + mesh_norm[v][1].powi(2) + mesh_norm[v][2].powi(2)).sqrt();
        if nl > 1e-12 {
            mesh_norm[v][0] /= nl;
            mesh_norm[v][1] /= nl;
            mesh_norm[v][2] /= nl;
        }
    }

    // Detect vertices already lying on a sharp 2-face CAD edge vs single-face vertices,
    // and evaluate outward CAD face normals g_v.
    let mut cad_norm = vec![[0.0f64; 3]; nv];
    let mut has_cad_norm = vec![false; nv];
    let mut locked_on_edge = vec![false; nv];
    let probe_eps = 0.012 * h;
    let dirs: [[f64; 3]; 6] = [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ];
    for v in 0..nv {
        if !is_bnd[v] {
            continue;
        }
        let p = lin.verts[v];
        let mn = mesh_norm[v];
        // Sample slightly outside along mesh_norm so flat-cap boundary ties never flip sign
        let p_out = [
            p[0] + 1e-4 * h * mn[0],
            p[1] + 1e-4 * h * mn[1],
            p[2] + 1e-4 * h * mn[2],
        ];
        let d = expr::eval(body, p_out);
        let gl = (d.g[0].powi(2) + d.g[1].powi(2) + d.g[2].powi(2)).sqrt();
        if gl > 0.25 {
            let mut g = [d.g[0] / gl, d.g[1] / gl, d.g[2] / gl];
            if g[0] * mn[0] + g[1] * mn[1] + g[2] * mn[2] < -0.1 {
                g = [-g[0], -g[1], -g[2]];
            }
            cad_norm[v] = g;
            has_cad_norm[v] = true;
        }
        if !is_on_datum_plane(p) {
            continue;
        }
        // Check if p already lies at the intersection of two CAD faces within 0.008 * h
        let mut near_planes: Vec<[f64; 3]> = Vec::new();
        for dir in &dirs {
            let qp = [
                p[0] + probe_eps * dir[0],
                p[1] + probe_eps * dir[1],
                p[2] + probe_eps * dir[2],
            ];
            let dq = expr::eval(body, qp);
            let qgl = (dq.g[0].powi(2) + dq.g[1].powi(2) + dq.g[2].powi(2)).sqrt();
            if qgl > 0.5 {
                let qg = [dq.g[0] / qgl, dq.g[1] / qgl, dq.g[2] / qgl];
                let plane_dist = ((p[0] - qp[0]) * qg[0]
                    + (p[1] - qp[1]) * qg[1]
                    + (p[2] - qp[2]) * qg[2])
                    + dq.v / qgl;
                if plane_dist.abs() < 0.008 * h {
                    near_planes.push(qg);
                }
            }
        }
        'outer: for i in 0..near_planes.len() {
            for j in (i + 1)..near_planes.len() {
                let dot = near_planes[i][0] * near_planes[j][0]
                    + near_planes[i][1] * near_planes[j][1]
                    + near_planes[i][2] * near_planes[j][2];
                if dot > -0.35 && dot < 0.70 {
                    locked_on_edge[v] = true;
                    break 'outer;
                }
            }
        }
    }

    // Build vertex -> incident tets map for volume & orientation guarding
    let mut v_tets: Vec<Vec<usize>> = vec![Vec::new(); nv];
    for (ti, t) in lin.tets.iter().enumerate() {
        for &vi in t {
            v_tets[vi as usize].push(ti);
        }
    }

    let init_locked = locked_on_edge.clone();
    let orig_verts = lin.verts.clone();
    let tiny = 1e-9 * h;
    for u in 0..nv {
        if !is_bnd[u] || !has_cad_norm[u] || init_locked[u] {
            continue;
        }
        let pu = orig_verts[u];
        let g1 = cad_norm[u];
        let mut best_cand: Option<([f64; 3], f64)> = None;
        for &w in &adj[u] {
            if !has_cad_norm[w] {
                continue;
            }
            let pw = orig_verts[w];
            let g2 = cad_norm[w];
            let c = g1[0] * g2[0] + g1[1] * g2[1] + g1[2] * g2[2];
            if !(-0.35..0.70).contains(&c) {
                continue;
            }
            let det = 1.0 - c * c;
            if det < 0.25 {
                continue;
            }
            let dx = pw[0] - pu[0];
            let dy = pw[1] - pu[1];
            let dz = pw[2] - pu[2];
            let delta_u = g2[0] * dx + g2[1] * dy + g2[2] * dz;
            let delta_w = -(g1[0] * dx + g1[1] * dy + g1[2] * dz);
            // Only snap the endpoint that is strictly closer to the 2-plane intersection line,
            // when w sits genuinely inside the adjacent face (delta_w > 0.22 * h), and only for
            // convex outward rims (delta_u > 0) within 0.56 * h.
            if delta_u <= 1e-5 * h
                || delta_u > 0.56 * h
                || delta_w.abs() <= 0.22 * h
                || delta_u >= delta_w.abs() - 1e-5 * h
            {
                continue;
            }
            let inv_sqrt_det = 1.0 / det.sqrt();
            let t1 = [
                (g2[0] - c * g1[0]) * inv_sqrt_det,
                (g2[1] - c * g1[1]) * inv_sqrt_det,
                (g2[2] - c * g1[2]) * inv_sqrt_det,
            ];
            let s_lin = delta_u * inv_sqrt_det;
            if s_lin > 0.58 * h {
                continue;
            }
            // Only snap onto bounding-box datum caps (where `stuff()`'s zero-patch warping has
            // already placed the cap's lattice vertices on the exact datum plane, so snapping
            // the few diagonal-cut rim vertices completes a flat, watertight cap rim without
            // erecting isolated spikes on off-lattice interior bevel strips) or near-flat plateaus.
            let p_lin = [
                pu[0] + s_lin * t1[0],
                pu[1] + s_lin * t1[1],
                pu[2] + s_lin * t1[2],
            ];
            let is_near_flat_plateau = g2[2].abs() > 0.99 && s_lin <= 0.16 * h;
            if !is_on_datum_plane(p_lin) && !is_near_flat_plateau {
                continue;
            }
            let s_max = (1.18 * s_lin).min(0.62 * h);
            let p_far = [
                pu[0] + s_max * t1[0],
                pu[1] + s_max * t1[1],
                pu[2] + s_max * t1[2],
            ];
            if expr::eval(body, p_far).v <= 1e-4 * h {
                continue;
            }
            let mut s_lo = 0.0f64;
            let mut s_hi = s_max;
            for _ in 0..32 {
                let s_mid = 0.5 * (s_lo + s_hi);
                let pm = [
                    pu[0] + s_mid * t1[0],
                    pu[1] + s_mid * t1[1],
                    pu[2] + s_mid * t1[2],
                ];
                if expr::eval(body, pm).v > tiny {
                    s_hi = s_mid;
                } else {
                    s_lo = s_mid;
                }
            }
            let s_end = 0.5 * (s_lo + s_hi);
            if (s_end - s_lin).abs() > 0.06 * h {
                continue;
            }
            let p_new = [
                pu[0] + s_end * t1[0],
                pu[1] + s_end * t1[1],
                pu[2] + s_end * t1[2],
            ];
            if !is_on_datum_plane(p_new) && !is_near_flat_plateau {
                continue;
            }
            let step_dist = s_end;
            if step_dist <= 0.58 * h && best_cand.is_none_or(|(_, d)| step_dist < d) {
                best_cand = Some((p_new, step_dist));
            }
        }
        if let Some((p_new, _)) = best_cand {
            let old_p = lin.verts[u];
            let old_vols: Vec<f64> = v_tets[u]
                .iter()
                .map(|&ti| tet_signed_vol(&lin.verts, lin.tets[ti]))
                .collect();
            lin.verts[u] = p_new;
            let ok = v_tets[u].iter().zip(old_vols.iter()).all(|(&ti, &ov)| {
                let nv_vol = tet_signed_vol(&lin.verts, lin.tets[ti]);
                nv_vol > 1e-12 && nv_vol >= 0.05 * ov
            });
            if !ok {
                lin.verts[u] = old_p;
            } else {
                locked_on_edge[u] = true;
            }
        }
    }

    // Pass 2: Snap chamfered vertices lying on horizontal step/cap planes (`|g1_z| > 0.99`,
    // `f(pu) ≈ 0`) radially/horizontally outward onto the exact outer circular rim when the
    // horizontal face terminates at an outer vertical/cylindrical wall within `0.60 * h`
    // (or within `0.04 * h` for lattice-graze corners on the top datum cap).
    for u in 0..nv {
        if !is_bnd[u] || !has_cad_norm[u] || locked_on_edge[u] {
            continue;
        }
        let pu = lin.verts[u];
        let on_datum = is_on_datum_plane(pu);
        if cad_norm[u][2].abs() <= 0.99 {
            continue;
        }
        if on_datum && (pu[2] - b_hi[2]).abs() > 0.03 * h {
            continue;
        }
        if expr::eval(body, pu).v.abs() > 1e-3 * h {
            continue;
        }
        let mn = mesh_norm[u];
        let mh = (mn[0] * mn[0] + mn[1] * mn[1]).sqrt();
        let r = (pu[0] * pu[0] + pu[1] * pu[1]).sqrt();
        let mut dirs_2d: Vec<[f64; 3]> = Vec::new();
        if r > 1e-6 {
            dirs_2d.push([pu[0] / r, pu[1] / r, 0.0]);
        }
        if mh > 0.20 {
            dirs_2d.push([mn[0] / mh, mn[1] / mh, 0.0]);
        }
        let s_max = if on_datum { 0.04 * h } else { 0.62 * h };
        let mut best_s: Option<([f64; 3], f64)> = None;
        for t in dirs_2d {
            let p_near = [
                pu[0] + 0.005 * h * t[0],
                pu[1] + 0.005 * h * t[1],
                pu[2],
            ];
            let p_far = [
                pu[0] + s_max * t[0],
                pu[1] + s_max * t[1],
                pu[2],
            ];
            if expr::eval(body, p_near).v.abs() > 1e-4 * h || expr::eval(body, p_far).v <= 1e-4 * h {
                continue;
            }
            let mut s_lo = 0.0f64;
            let mut s_hi = s_max;
            for _ in 0..32 {
                let s_mid = 0.5 * (s_lo + s_hi);
                let pm = [pu[0] + s_mid * t[0], pu[1] + s_mid * t[1], pu[2]];
                if expr::eval(body, pm).v > tiny {
                    s_hi = s_mid;
                } else {
                    s_lo = s_mid;
                }
            }
            let s_end = 0.5 * (s_lo + s_hi);
            if s_end <= 1e-5 * h || s_end > 0.98 * s_max {
                continue;
            }
            let p_probe = [
                pu[0] + (s_end + 1e-3 * h) * t[0],
                pu[1] + (s_end + 1e-3 * h) * t[1],
                pu[2],
            ];
            let dg = expr::eval(body, p_probe).g;
            let dgl = (dg[0] * dg[0] + dg[1] * dg[1] + dg[2] * dg[2]).sqrt();
            if dgl < 0.5 || (dg[2] / dgl).abs() > 0.25 {
                continue;
            }
            if best_s.is_none_or(|(_, d)| s_end < d) {
                best_s = Some((t, s_end));
            }
        }
        if let Some((t, s_end)) = best_s {
            let old_p = lin.verts[u];
            let old_vols: Vec<f64> = v_tets[u]
                .iter()
                .map(|&ti| tet_signed_vol(&lin.verts, lin.tets[ti]))
                .collect();
            for frac in [1.0, 0.85, 0.70, 0.55] {
                if on_datum && frac < 1.0 {
                    break;
                }
                let s = frac * s_end;
                lin.verts[u] = [pu[0] + s * t[0], pu[1] + s * t[1], pu[2]];
                let ok = v_tets[u].iter().zip(old_vols.iter()).all(|(&ti, &ov)| {
                    let nv_vol = tet_signed_vol(&lin.verts, lin.tets[ti]);
                    nv_vol > 1e-12 && nv_vol >= 0.05 * ov
                });
                if ok {
                    if frac == 1.0 {
                        locked_on_edge[u] = true;
                    }
                    break;
                }
                lin.verts[u] = old_p;
            }
        }
    }
    locked_on_edge
}

/// Project P2 boundary mid-edge nodes along curved 2-face datum-cap rims so circular/curved cap
/// boundaries are faithfully represented without popping diagonal bevel midpoints out of plane.
fn project_p2_boundary_midpoints(
    mesh: &mut P2Mesh,
    facets: &[Facet],
    locked_on_edge: &[bool],
    body: &expr::Expr,
    h: f64,
) {
    // Compute unit normal per boundary facet and map each mid-edge node to its incident facet normals.
    let mut mid_facets: std::collections::HashMap<usize, Vec<[f64; 3]>> =
        std::collections::HashMap::with_capacity(facets.len() * 2);
    for fct in facets {
        let p0 = mesh.nodes[fct.nodes[0] as usize];
        let p1 = mesh.nodes[fct.nodes[1] as usize];
        let p2 = mesh.nodes[fct.nodes[2] as usize];
        let ux = p1[0] - p0[0];
        let uy = p1[1] - p0[1];
        let uz = p1[2] - p0[2];
        let vx = p2[0] - p0[0];
        let vy = p2[1] - p0[1];
        let vz = p2[2] - p0[2];
        let nx = uy * vz - uz * vy;
        let ny = uz * vx - ux * vz;
        let nz = ux * vy - uy * vx;
        let nl = (nx * nx + ny * ny + nz * nz).sqrt();
        if nl > 1e-12 {
            let fn_unit = [nx / nl, ny / nl, nz / nl];
            for &vm in &fct.nodes[3..6] {
                mid_facets.entry(vm as usize).or_default().push(fn_unit);
            }
        }
    }

    let mut visited = vec![false; mesh.nodes.len()];
    let tiny = 1e-9 * h;
    for fct in facets {
        let corners = [fct.nodes[0] as usize, fct.nodes[1] as usize, fct.nodes[2] as usize];
        let mids = [fct.nodes[3] as usize, fct.nodes[4] as usize, fct.nodes[5] as usize];
        for (k, &vm) in mids.iter().enumerate() {
            if vm >= visited.len() || visited[vm] {
                continue;
            }
            visited[vm] = true;
            let va = corners[k];
            let vb = corners[(k + 1) % 3];
            let pa = mesh.nodes[va];
            let pb = mesh.nodes[vb];
            let pm = mesh.nodes[vm];
            let ex = pb[0] - pa[0];
            let ey = pb[1] - pa[1];
            let ez = pb[2] - pa[2];
            let edge_len = (ex * ex + ey * ey + ez * ez).sqrt();
            if edge_len < 1e-9 {
                continue;
            }
            let is_crease_edge = va < locked_on_edge.len()
                && vb < locked_on_edge.len()
                && locked_on_edge[va]
                && locked_on_edge[vb]
                && mid_facets.get(&vm).is_some_and(|ns| {
                    ns.len() >= 2
                        && (ns[0][0] * ns[1][0] + ns[0][1] * ns[1][1] + ns[0][2] * ns[1][2]) < 0.70
                });
            let d = expr::eval(body, pm);
            let gl = (d.g[0].powi(2) + d.g[1].powi(2) + d.g[2].powi(2)).sqrt();
            if is_crease_edge && d.v.abs() <= 1e-3 * h && gl > 0.25 {
                // Both endpoints sit on a sharp 2-face datum-cap crease edge, and the chord
                // midpoint fell onto the cap face (e.g. flat top cap inside a circular cylinder
                // rim). Bisect in-plane along the chord normal toward the curved rim boundary.
                let gm = [d.g[0] / gl, d.g[1] / gl, d.g[2] / gl];
                let e_unit = [ex / edge_len, ey / edge_len, ez / edge_len];
                let tx = e_unit[1] * gm[2] - e_unit[2] * gm[1];
                let ty = e_unit[2] * gm[0] - e_unit[0] * gm[2];
                let tz = e_unit[0] * gm[1] - e_unit[1] * gm[0];
                let tl = (tx * tx + ty * ty + tz * tz).sqrt();
                if tl > 0.25 {
                    let t_perp = [tx / tl, ty / tl, tz / tl];
                    let s_max = (0.20 * edge_len).min(0.35 * h);
                    for sign in [1.0f64, -1.0f64] {
                        let dir = [sign * t_perp[0], sign * t_perp[1], sign * t_perp[2]];
                        let p_far = [
                            pm[0] + s_max * dir[0],
                            pm[1] + s_max * dir[1],
                            pm[2] + s_max * dir[2],
                        ];
                        let p_opp = [
                            pm[0] - s_max * dir[0],
                            pm[1] - s_max * dir[1],
                            pm[2] - s_max * dir[2],
                        ];
                        if expr::eval(body, p_far).v > 1e-4 * h
                            && expr::eval(body, p_opp).v <= 1e-5 * h
                        {
                            let mut s_lo = 0.0f64;
                            let mut s_hi = s_max;
                            for _ in 0..28 {
                                let s_mid = 0.5 * (s_lo + s_hi);
                                let q = [
                                    pm[0] + s_mid * dir[0],
                                    pm[1] + s_mid * dir[1],
                                    pm[2] + s_mid * dir[2],
                                ];
                                if expr::eval(body, q).v > tiny {
                                    s_hi = s_mid;
                                } else {
                                    s_lo = s_mid;
                                }
                            }
                            let s_end = 0.5 * (s_lo + s_hi);
                            mesh.nodes[vm] = [
                                pm[0] + s_end * dir[0],
                                pm[1] + s_end * dir[1],
                                pm[2] + s_end * dir[2],
                            ];
                            break;
                        }
                    }
                }
            }
        }
    }
}

fn prepare(
    doc: &Document,
    res: Resolution,
    clock: Clock,
    need_loads: bool,
) -> Result<Prepared, SetupError> {
    let sdoc = structural_doc(doc);
    let doc = sdoc.as_ref();
    let t0 = clock();
    let body = expr::compile_body(doc);
    let b = expr::bounds(&body);
    if b.is_empty() {
        return Err(SetupError("the model is empty".into()));
    }
    let has_thermal = !doc.temperatures.is_empty()
        || !doc.convections.is_empty()
        || !doc.heat_fluxes.is_empty();
    if doc.supports.is_empty() && (!has_thermal || !doc.loads.is_empty()) {
        return Err(SetupError(
            "add a support: the part needs something to hold it".into(),
        ));
    }
    if need_loads
        && doc.loads.is_empty()
        && !has_thermal
    {
        return Err(SetupError("add a load".into()));
    }
    let mat = material_of(doc)?;
    let vol_mm3 = fieldsmith_field::volume::integrate(
        &body,
        fieldsmith_field::volume::default_spacing(&b) * 2.0,
        None,
    );
    let h = match res {
        Resolution::Size(h) => h,
        Resolution::Dofs(n) => size_for_dofs(vol_mm3.max(1e-9), n),
    };
    let f = |p: [f64; 3]| expr::eval(&body, p).v;
    let (mut lin, _) = stuff(f, b.lo, b.hi, h);
    lin.compact();
    prune_boundary_ear_tets(&mut lin, b.lo, b.hi, h);
    if lin.tets.is_empty() {
        return Err(SetupError(
            "the part is too thin to mesh at this resolution".into(),
        ));
    }
    let locked_on_edge = snap_feature_edges(&mut lin, &body, b.lo, b.hi, h);
    let quality = fieldsmith_mesh::quality(&lin);
    let mut mesh = P2Mesh::from_linear(&lin.verts, &lin.tets);
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
    project_p2_boundary_midpoints(&mut mesh, &facets, &locked_on_edge, &body, h);
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

    // Supports and per-volume material assignments (V53).
    let fixed = fixed_mask(doc, &mesh, &facets, h)?;
    let mut labelled = Vec::new();
    collect_labelled_volumes(doc, &doc.root, &[], &mut labelled)?;
    let mut young = vec![mat.e; mesh.tets.len()];
    let mut yield_elem = vec![mat.yield_strength; mesh.tets.len()];
    let mut k_elem = vec![1.0; mesh.tets.len()];
    let mut alpha_elem = vec![mat.alpha; mesh.tets.len()];
    let mut rho_avg = mat.density;
    let mut mass_g = vol_mm3 * mat.density / 1000.0;
    if !labelled.is_empty() {
        let base_k = mat.k.unwrap_or(1.0).max(1e-9);
        let mut weighted_rho = 0.0f64;
        let mut total_tet_vol = 0.0f64;
        for (e, tet) in mesh.tets.iter().enumerate() {
            let v0 = mesh.nodes[tet[0] as usize];
            let v1 = mesh.nodes[tet[1] as usize];
            let v2 = mesh.nodes[tet[2] as usize];
            let v3 = mesh.nodes[tet[3] as usize];
            let c = [
                0.25 * (v0[0] + v1[0] + v2[0] + v3[0]),
                0.25 * (v0[1] + v1[1] + v2[1] + v3[1]),
                0.25 * (v0[2] + v1[2] + v2[2] + v3[2]),
            ];
            let mut m_e = &mat;
            for (_, vexpr, vmat) in &labelled {
                if expr::eval(vexpr, c).v <= 1e-6 * h {
                    m_e = vmat;
                }
            }
            young[e] = m_e.e;
            yield_elem[e] = m_e.yield_strength;
            k_elem[e] = m_e.k.unwrap_or(base_k) / base_k;
            alpha_elem[e] = m_e.alpha;
            let tv = mesh.tet_volume(e).abs();
            weighted_rho += tv * m_e.density;
            total_tet_vol += tv;
        }
        if total_tet_vol > 0.0 {
            rho_avg = weighted_rho / total_tet_vol;
            mass_g = vol_mm3 * rho_avg / 1000.0;
        }
    }
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
        yield_elem,
        k_elem,
        alpha_elem,
        rho_avg,
        mass_g,
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
        rho_avg,
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
        rho_avg * 1e-9,
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
    let sdoc = structural_doc(doc);
    let doc = sdoc.as_ref();
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
        yield_elem,
        k_elem,
        alpha_elem,
        rho_avg,
        mass_g,
        h,
        quality,
        mesh_ms,
    } = prepare(doc, res, clock, modes.is_none())?;
    // `prepare` has just returned, so `clock` is its meshing end; its t0 is mesh_ms back.
    let t_mesh = clock();
    let t0 = t_mesh - mesh_ms;
    let nn = mesh.nodes.len();
    let mut cases = Vec::new();
    let mut iterations = 0;
    let nl_plastic = doc
        .lets
        .iter()
        .find(|l| l.name == "nonlinear_plastic")
        .map(|l| l.value)
        .unwrap_or(0.0)
        > 0.5;
    let hardening_n = doc
        .lets
        .iter()
        .find(|l| l.name == "hardening_n")
        .map(|l| l.value)
        .unwrap_or(0.18);
    for name in case_names(doc) {
        let (force, reports, prescribed_map) = case_force_report(doc, &mesh, &facets, &name, h)?;
        let mut node_key: std::collections::HashMap<[u64; 3], u8> =
            std::collections::HashMap::new();
        for (i, p) in mesh.nodes.iter().enumerate() {
            node_key.insert([p[0].to_bits(), p[1].to_bits(), p[2].to_bits()], fixed[i]);
        }
        let prescribed = prescribed_fn(&node_key, &prescribed_map);
        let mut case = if nl_plastic && (mat.yield_strength > 0.0 || yield_elem.iter().any(|&y| y > 0.0)) {
            run_case_elastoplastic(
                &mesh,
                &young,
                mat.nu,
                mat.yield_strength,
                &yield_elem,
                hardening_n,
                &fixed,
                force,
                name,
                equilibrate,
                &prescribed,
            )?
        } else {
            run_case(
                &mesh,
                &Material::iso(young.to_vec(), mat.nu),
                &fixed,
                force,
                name,
                equilibrate,
                &prescribed,
            )?
        };
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
        let has_supports = fixed.iter().any(|&m| m != 0);
        let mut alphas = Vec::with_capacity(alpha_elem.len());
        if has_supports {
            let _ = mat.alpha.ok_or_else(|| {
                SetupError(format!(
                    "The thermal flow needs a thermal-expansion coefficient and material '{}' \
                     states none — pick a material whose sheet states a CTE, or give a custom \
                     material an a= value",
                    doc.material
                ))
            })?;
            for &a in &alpha_elem {
                let av = a.ok_or_else(|| {
                    SetupError(
                        "The thermal flow needs a thermal-expansion coefficient on every assigned \
                         volume material — pick a material whose sheet states a CTE, or give a custom \
                         material an a= value"
                            .into(),
                    )
                })?;
                alphas.push(av);
            }
        } else {
            alphas.resize(alpha_elem.len(), 0.0);
        }
        let t_thermal = clock();
        for name in thermal_cases {
            // Thermal cases prescribe no boundary traction; the expansion pseudo-load is
            // the body, the supports' reaction tractions are free.
            let case = run_thermal_case_multi(
                doc,
                &mesh,
                &young,
                mat.nu,
                &alphas,
                &fixed,
                &facets,
                name,
                h,
                &k_elem,
                equilibrate,
            )?;
            iterations += case.solve.iterations;
            cases.push(case);
        }
        thermal_ms = clock() - t_thermal;
    }
    if cases.is_empty() && modes.is_some() {
        let ne = mesh.tets.len();
        cases.push(CaseResult {
            name: "Modal".into(),
            u: vec![0.0; nn * 3],
            stress: vec![Stress::default(); nn],
            von_mises: vec![0.0; nn],
            vm_elem: vec![0.0; ne],
            sigma_min_elem: vec![0.0; ne],
            stress_elem: vec![Stress::default(); ne],
            plastic_strain_elem: vec![0.0; ne],
            grad_t_nodal: None,
            peel_nodal: Vec::new(),
            peel_elem: vec![0.0; ne],
            max_layer_peel: 0.0,
            max_von_mises: 0.0,
            max_vm_elem: 0.0,
            max_displacement: 0.0,
            compliance: 0.0,
            compliance_upper: None,
            solve: SolveReport {
                dofs: nn * 3,
                free_dofs: nn * 3,
                coarse_dofs: 0,
                iterations: 0,
                rel_residual: 0.0,
                converged: true,
                ..Default::default()
            },
            load_reports: Vec::new(),
            temperature: None,
            conduction: None,
        });
    }
    // Inter-layer peel utilization across all cases for the document's build orientation
    // (`solve build=(theta, phi)`, default `+Z`), plus a hemisphere orientation sweep.
    let (yield_z, shear_z) = mat.effective_layer_strengths();
    let build_angles = doc.solve.build.unwrap_or([0.0, 0.0]);
    let b_hat = crate::overhang::build_axis_from_angles(build_angles[0], build_angles[1]);
    for c in &mut cases {
        populate_case_peel(c, b_hat, yield_z, shear_z);
    }
    let (best_build_angles, min_peak_peel, worst_build_angles, max_peak_peel) =
        sweep_build_orientations(&cases, yield_z, shear_z, None);

    // Envelope over every case — mechanical and thermal: the tracked readout is the
    // worst stress the part can see from any of them.
    let envelope = (0..nn)
        .map(|i| cases.iter().map(|c| c.von_mises[i]).fold(0.0, f64::max))
        .collect();
    let envelope_elem = (0..mesh.tets.len())
        .map(|e| cases.iter().map(|c| c.vm_elem[e]).fold(0.0, f64::max))
        .collect();
    let peel_envelope_elem = (0..mesh.tets.len())
        .map(|e| cases.iter().map(|c| c.peel_elem[e]).fold(0.0, f64::max))
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
                rho_avg * 1e-9,
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
        mass_g,
        yield_strength: mat.yield_strength,
        yield_z,
        shear_z,
        build_angles,
        best_build_angles,
        min_peak_peel,
        worst_build_angles,
        max_peak_peel,
        cases,
        envelope,
        envelope_elem,
        peel_envelope_elem,
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
        // The certified ordering holds up to the witness slack the direct solve carries
        // (measured 1.0e-6 relative in-span): an engineering certificate, not a
        // floating-point identity.
        assert!(
            up >= case.compliance - 1e-4 * want,
            "bracket ordering: upper {up} vs lower {} (want {want})",
            case.compliance
        );
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
        let gt = th.grad_t_nodal.as_ref().expect("nodal temperature gradient");
        assert_eq!(gt.len(), a.mesh.nodes.len() * 3);
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
            assert!(
                (gt[3 * n] - 2.0).abs() < 1e-5
                    && gt[3 * n + 1].abs() < 1e-5
                    && gt[3 * n + 2].abs() < 1e-5,
                "node {} grad_t = ({:.5}, {:.5}, {:.5}), expected (2, 0, 0)",
                n,
                gt[3 * n],
                gt[3 * n + 1],
                gt[3 * n + 2]
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

    #[test]
    fn elastoplastic_secant_redistributes_notch_stress_and_increases_deflection() {
        let linear_fsm = "\
name \"Notched Clinch Neck Strip\"
let nonlinear_plastic = 0
let hardening_n = 0.18
custommaterial al5754 e=70000 nu=0.33 yield=110 density=2.67
material al5754
group g {
  box strip size=(24, 8, 4) at=(12, 4, 2)
  cylinder notch r=2.2 h=8 at=(12, 0, 2) subtract
}
region root_face = strip.nx
region tip_face = strip.px
support clamp root_face fixed=xyz
load pull tip_face force=(4200, 0, 0)
";
        let d_lin = parse(linear_fsm);
        let a_lin = analyse(&d_lin, Resolution::Size(2.0), now).expect("linear analyse");
        let c_lin = &a_lin.cases[0];
        assert!(
            c_lin.max_vm_elem > 110.0,
            "linear notch stress must exceed yield (110 MPa), got {}",
            c_lin.max_vm_elem
        );

        let nl_fsm = linear_fsm.replace("let nonlinear_plastic = 0", "let nonlinear_plastic = 1");
        let d_nl = parse(&nl_fsm);
        let a_nl = analyse(&d_nl, Resolution::Size(2.0), now).expect("nonlinear analyse");
        let c_nl = &a_nl.cases[0];

        // J2 secant hardening softens yielded elements (E_sec < E_0), increasing plastic deflection
        // and total PCG iterations relative to the single linear pass.
        assert!(
            c_nl.max_displacement > c_lin.max_displacement,
            "nonlinear elasto-plastic displacement ({}) must exceed linear elastic displacement ({})",
            c_nl.max_displacement,
            c_lin.max_displacement
        );
        assert!(
            c_nl.solve.iterations > c_lin.solve.iterations,
            "secant passes must accumulate iterations ({} vs {})",
            c_nl.solve.iterations,
            c_lin.solve.iterations
        );
        assert!(
            c_lin.plastic_strain_elem.iter().all(|&p| p == 0.0),
            "linear case must have zero plastic strain"
        );
        let max_ep = c_nl.plastic_strain_elem.iter().cloned().fold(0.0, f64::max);
        assert!(
            max_ep > 0.0,
            "nonlinear case must record positive equivalent plastic strain (%), got {max_ep}"
        );
    }

    #[test]
    fn t19_and_a19_clinching_models_solve_at_coarse_and_medium() {
        let t19_src = include_str!("../../../examples/tutorials/T19-sheet-metal-clinching-slock-nonlinear-fem.fsm");
        let a19_src = include_str!("../../../examples/analytic/A19-sheet-metal-clinching-lee-kim-breda.fsm");
        // Also verify the legacy round=0.8 thin-edge variant at mobile COARSE_DOFS (12_000)
        let legacy_rounded = "\
name \"Legacy T19 Rounded\"
custommaterial al5754_clinch e=70000 nu=0.33 a=2.3e-5 yield=65 density=2.67
material al5754_clinch
box lower_sheet size=(24, 22, 3.0) at=(0, 0, 1.5) round=0.8
cylinder die_button r=4.6 h=2.4 at=(0, 0, -0.4)
cylinder upper_boss r=4.2 h=2.6 at=(0, 0, 3.4)
cylinder punch_pit r=2.6 h=3.85 at=(0, 0, 3.55) subtract
cylinder die_anvil_recess r=2.2 h=0.8 at=(0, 0, -1.4) subtract
region lower_clamp = lower_sheet.nx + lower_sheet.px
region upper_pull_ring = upper_boss.top
support die_fixture lower_clamp fixed=xyz
load cross_tension upper_pull_ring force=(0, 0, 2400)
";
        let d_leg = parse(legacy_rounded);
        let r_leg = analyse(&d_leg, Resolution::Dofs(12_000), now)
            .expect("legacy rounded T19 at 12_000 DOFs");
        assert!(r_leg.cases[0].solve.converged);

        for (label, src) in [("T19", t19_src), ("A19", a19_src)] {
            let doc = fsm::parse(src).expect("valid .fsm file");
            let res_coarse = analyse(&doc, Resolution::Dofs(12_000), now)
                .unwrap_or_else(|e| panic!("{label} failed at Coarse (12k): {e:?}"));
            assert!(res_coarse.cases[0].solve.converged, "{label} coarse converged");
            assert!(res_coarse.cases[0].max_displacement > 0.0, "{label} coarse displacement > 0");
            let res_med = analyse(&doc, Resolution::Dofs(25_000), now)
                .unwrap_or_else(|e| panic!("{label} failed at Medium (25k): {e:?}"));
            assert!(res_med.cases[0].solve.converged, "{label} medium converged");
            assert!(res_med.cases[0].max_vm_elem > 10.0, "{label} medium vm > 10 MPa");
        }
    }

    #[test]
    fn t15_spool_collar_rim_nodes_lie_on_exact_edges_and_surfaces() {
        let t15_src = include_str!("../../../examples/tutorials/T15-parametric-2d-sketch-constraints.fsm");
        let doc = fsm::parse(t15_src).expect("valid T15");
        let body = expr::compile_body(&doc);
        let b = expr::bounds(&body);
        assert_eq!(b.lo, [-27.0, -18.0, 0.0]);
        assert_eq!(b.hi, [27.0, 18.0, 33.0]);

        let p = prepare(&doc, Resolution::Dofs(40_000), now, true).expect("prepare T15");
        let n_lin = p.surface.iter().flat_map(|f| f[0..3].iter()).map(|&i| i as usize).max().unwrap_or(0) + 1;
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); p.mesh.nodes.len()];
        for f in &p.surface {
            let [i0, i1, i2] = [f[0] as usize, f[1] as usize, f[2] as usize];
            for &(a, b) in &[(i0, i1), (i1, i2), (i2, i0)] {
                if !adj[a].contains(&b) {
                    adj[a].push(b);
                }
                if !adj[b].contains(&a) {
                    adj[b].push(a);
                }
            }
        }
        for (vi, pt) in p.mesh.nodes.iter().take(n_lin).enumerate() {
            let r = (pt[0] * pt[0] + pt[1] * pt[1]).sqrt();
            if (pt[2] - 27.5).abs() < 0.1 && r > 8.6 {
                assert!(
                    (r - 11.0).abs() < 1e-2,
                    "lower collar step vertex {vi} at z=27.5 must lie on r=11.0, got r={r:.4} ({pt:?})"
                );
            }
        }
        let deg3_count = (0..p.mesh.nodes.len())
            .filter(|&v| adj[v].len() == 3)
            .count();
        assert_eq!(
            deg3_count, 0,
            "T15 must have zero degree-3 boundary pyramid spikes, got {deg3_count}"
        );
        for (fi, f) in p.surface.iter().enumerate() {
            let p0 = p.mesh.nodes[f[0] as usize];
            let p1 = p.mesh.nodes[f[1] as usize];
            let p2 = p.mesh.nodes[f[2] as usize];
            let ux = p1[0] - p0[0];
            let uy = p1[1] - p0[1];
            let uz = p1[2] - p0[2];
            let vx = p2[0] - p0[0];
            let vy = p2[1] - p0[1];
            let vz = p2[2] - p0[2];
            let nx = uy * vz - uz * vy;
            let ny = uz * vx - ux * vz;
            let nz = ux * vy - uy * vx;
            let nl = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-12);
            let n_unit = [nx / nl, ny / nl, nz / nl];
            if p0[2] > 31.5 || p1[2] > 31.5 || p2[2] > 31.5 {
                assert!(
                    !(0.22..0.99).contains(&n_unit[2]),
                    "unexpected chamfer facet {fi} on T15 spool_collar top rim: n_z={:.3} p0={p0:?} p1={p1:?} p2={p2:?}",
                    n_unit[2]
                );
            }
            // Verify P2 mid-edge nodes on box bevels (z <= 27 mm) never pop out of the facet plane
            for (k, &vm) in f[3..6].iter().enumerate() {
                let va = f[k] as usize;
                let vb = f[(k + 1) % 3] as usize;
                let pa = p.mesh.nodes[va];
                let pb = p.mesh.nodes[vb];
                let pm = p.mesh.nodes[vm as usize];
                if pa[2] <= 27.0 && pb[2] <= 27.0 && (pa[0].abs() > 14.0 || pa[1].abs() > 10.0) {
                    let mid_lin = [
                        0.5 * (pa[0] + pb[0]),
                        0.5 * (pa[1] + pb[1]),
                        0.5 * (pa[2] + pb[2]),
                    ];
                    let tent_dev = ((pm[0] - mid_lin[0]).powi(2)
                        + (pm[1] - mid_lin[1]).powi(2)
                        + (pm[2] - mid_lin[2]).powi(2))
                    .sqrt();
                    assert!(
                        tent_dev < 1e-4,
                        "P2 mid-edge node {vm} on box bevel facet {fi} popped out by {tent_dev:.4} mm"
                    );
                }
                let ra = (pa[0] * pa[0] + pa[1] * pa[1]).sqrt();
                let rb = (pb[0] * pb[0] + pb[1] * pb[1]).sqrt();
                if (pa[2] - 33.0).abs() < 1e-3
                    && (pb[2] - 33.0).abs() < 1e-3
                    && (ra - 11.0).abs() < 1e-2
                    && (rb - 11.0).abs() < 1e-2
                {
                    let rm = (pm[0] * pm[0] + pm[1] * pm[1]).sqrt();
                    assert!(
                        (pm[2] - 33.0).abs() < 1e-3 && (rm - 11.0).abs() < 1e-2,
                        "P2 rim midpoint {vm} off outer collar circle: z={:.4}, r={:.4}",
                        pm[2],
                        rm
                    );
                }
            }
        }
    }
}
