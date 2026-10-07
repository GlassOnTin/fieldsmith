//! The analysis pipeline: document → field → tetrahedral mesh → boundary conditions → P2 solve →
//! nodal results. Runs identically natively (CLI, verification tests) and in a Web Worker.

pub mod fill;
pub mod fsmz;
pub mod import;
pub mod probe;
pub mod regions;
pub mod voxel;

use fieldsmith_doc::{materials, Document, LoadKind};
use fieldsmith_field::expr::{self, Expr};
use fieldsmith_mesh::{stuff, Quality};
use fieldsmith_solve::{
    element_stress, nodal_stress, solve, von_mises, Material, P2Mesh, Problem, SolveReport,
    SolverOptions, Stress,
};
use serde::Serialize;

pub use regions::{region_facets, Facet};

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

/// Nodal force vector for one load case: uniform traction over the case's facets, corners 0 and
/// each midside a/3. Shared by the solid and graded-fill solve paths, and by the verifier's
/// resolved-model runs.
pub fn case_force(
    doc: &Document,
    mesh: &P2Mesh,
    facets: &[Facet],
    name: &str,
    h: f64,
) -> Result<Vec<f64>, SetupError> {
    let mut force = vec![0.0; mesh.nodes.len() * 3];
    for l in doc.loads.iter().filter(|l| l.case == name) {
        let fs = region_facets(doc, &l.region, mesh, facets, h)?;
        if fs.is_empty() {
            return Err(SetupError(format!(
                "load '{}': region '{}' does not touch the part's surface",
                l.id, l.region
            )));
        }
        let area: f64 = fs.iter().map(|&i| facets[i].area(mesh)).sum();
        for &fi in &fs {
            let fct = &facets[fi];
            let a = fct.area(mesh);
            let t = match l.kind {
                LoadKind::Force { vector } => vector.map(|c| c / area),
                LoadKind::Pressure { mpa } => fct.normal(mesh).map(|c| -mpa * c),
            };
            // Uniform traction on a 6-node triangle: corners 0, each midside a/3.
            for &n in &fct.nodes[3..] {
                for c in 0..3 {
                    force[3 * n as usize + c] += t[c] * a / 3.0;
                }
            }
        }
    }
    Ok(force)
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
    })
}

pub fn analyse(doc: &Document, res: Resolution, clock: Clock) -> Result<Analysis, SetupError> {
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
    if doc.loads.is_empty() {
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
    let nn = mesh.nodes.len();
    let fixed = fixed_mask(doc, &mesh, &facets, h)?;

    let young = vec![mat.e; mesh.tets.len()];
    let mut cases = Vec::new();
    let mut iterations = 0;
    for name in case_names(doc) {
        let force = case_force(doc, &mesh, &facets, &name, h)?;
        let case = run_case(&mesh, &young, mat.nu, &fixed, force, name)?;
        iterations += case.solve.iterations;
        cases.push(case);
    }
    let envelope = (0..nn)
        .map(|i| cases.iter().map(|c| c.von_mises[i]).fold(0.0, f64::max))
        .collect();
    let envelope_elem = (0..mesh.tets.len())
        .map(|e| cases.iter().map(|c| c.vm_elem[e]).fold(0.0, f64::max))
        .collect();
    let t_end = clock();
    let provenance = Provenance {
        elements: mesh.tets.len(),
        order: 2,
        nodes: nn,
        dofs: nn * 3,
        element_size: h,
        mesh_ms: t_mesh - t0,
        solve_ms: t_end - t_mesh,
        total_ms: t_end - t0,
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
    })
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
}
