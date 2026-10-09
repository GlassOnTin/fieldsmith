//! WebAssembly bindings. Documents cross the boundary as JSON strings (the serde form of
//! [`fieldsmith_doc::Document`]); bulk numeric data crosses as typed arrays.

use fieldsmith_core::fill::{graded, Graded};
use fieldsmith_core::{analyse_full as core_analyse_full, Analysis, Provenance, Resolution};
use fieldsmith_doc::{fsm, materials, Document};
use fieldsmith_field::{expr, glsl, Expr};
use fieldsmith_mesh::adaptive::{surface_adaptive, AdaptiveOpts, Sample};
use fieldsmith_mesh::export::TriMesh;
use js_sys::{Float32Array, Float64Array, Uint16Array, Uint32Array, Uint8Array};
use serde::Serialize;
use wasm_bindgen::prelude::*;

/// Threaded build only: start the rayon pool on `n` Web Workers. JS calls
/// `await initThreadPool(n)` once after `init`; before it (and in the single-thread
/// build) rayon work runs on the calling thread.
#[cfg(all(feature = "threads", target_arch = "wasm32"))]
pub use wasm_bindgen_rayon::init_thread_pool;

/// Route Rust panics to the devtools console — wasm panics otherwise trap as a bare
/// `unreachable` with no message.
#[wasm_bindgen(start)]
fn start() {
    console_error_panic_hook::set_once();
}

fn js_err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

fn doc_from(json: &str) -> Result<Document, JsValue> {
    serde_json::from_str(json).map_err(|e| js_err(format!("invalid document: {e}")))
}

fn to_json<T: Serialize>(v: &T) -> Result<String, JsValue> {
    serde_json::to_string(v).map_err(js_err)
}

/// Parse `.fsm` text. Returns `{"ok": doc}` or `{"error": {"line", "message"}}`.
#[wasm_bindgen]
pub fn parse_fsm(text: &str) -> String {
    match fsm::parse(text) {
        Ok(d) => serde_json::json!({ "ok": d }).to_string(),
        Err(e) => {
            serde_json::json!({ "error": { "line": e.line, "message": e.message } }).to_string()
        }
    }
}

/// Print a document in canonical `.fsm` form.
#[wasm_bindgen]
pub fn print_fsm(doc_json: &str) -> Result<String, JsValue> {
    Ok(fsm::print(&doc_from(doc_json)?))
}

/// Validation problems (JSON array of strings; empty when valid).
#[wasm_bindgen]
pub fn validate(doc_json: &str) -> Result<String, JsValue> {
    to_json(&doc_from(doc_json)?.validate())
}

/// Compile a load-density `fn` and sample it for the setup editor (M11). `lo`/`hi` are the
/// requested sample range in face-local mm, used only when the fn has no bands (a plain
/// body); the fn's own band domain is used otherwise. Samples sit at in-plane y = 0 — a fn
/// of both x and y previews its x slice. The integral is the trapezoid over those samples,
/// per 1 mm of face width; the applied resultant uses the real face areas. Checks list the
/// document validate() strings this fn would raise.
#[wasm_bindgen(js_name = loadFnProbe)]
pub fn load_fn_probe(src: &str, lo: f64, hi: f64) -> Result<String, JsValue> {
    let f = match fieldsmith_doc::loaddist::LoadFn::compile(src) {
        Ok(f) => f,
        Err(e) => {
            return to_json(&serde_json::json!({ "ok": false, "error": e }));
        }
    };
    let (d, from): ((f64, f64), &str) = match f.domain() {
        Some(d) => (d, "bands"),
        None => ((lo, hi), "requested"),
    };
    let n = 121usize;
    let mut xs = Vec::with_capacity(n);
    let mut q = Vec::with_capacity(n);
    for i in 0..n {
        let x = d.0 + (d.1 - d.0) * i as f64 / (n - 1).max(1) as f64;
        xs.push(x);
        q.push(f.eval(x, 0.0));
    }
    // Per-1-mm-of-width integral of the sampled curve (the applied total scales with the
    // real facet areas; this is the shape-check number only).
    let mut integral = 0.0f64;
    for i in 1..n {
        integral += 0.5 * (q[i - 1] + q[i]) * (xs[i] - xs[i - 1]);
    }
    to_json(&serde_json::json!({
        "ok": true,
        "domain": { "lo": d.0, "hi": d.1, "from": from },
        "bands": f.bands().iter().map(|b| serde_json::json!({ "lo": b.lo, "hi": b.hi })).collect::<Vec<_>>(),
        "xs": xs,
        "q": q,
        "integral": integral,
        "checks": f.static_checks("load"),
    }))
}

/// The built-in material library (JSON array).
#[wasm_bindgen]
pub fn material_library() -> Result<String, JsValue> {
    to_json(&materials::library())
}

#[derive(Serialize)]
struct ShaderOut {
    source: String,
    bounds: Option<[[f64; 3]; 2]>,
    /// `vec2 fs_ident(vec3 p)` — (leaf index, face code) of the nearest active leaf surface.
    ident: String,
    /// Leaf ids in `fs_ident` index order.
    leaves: Vec<String>,
    /// Mesh texture specs to bind as `u_mesh{slot}` / `u_mesh{slot}p`.
    meshes: Vec<MeshTexOut>,
}

/// One mesh texture spec for the JS side. The registry key travels as 16 hex digits
/// because u64 numbers lose precision beyond 2^53 in JavaScript; `key_from_hex` reads
/// them back.
#[derive(Serialize)]
struct MeshTexOut {
    slot: usize,
    key_hex: String,
    dims: [usize; 3],
    origin: [f64; 3],
    spacing: f64,
    patch: bool,
}

fn mesh_tex_out(t: &glsl::MeshTex) -> MeshTexOut {
    MeshTexOut {
        slot: t.slot,
        key_hex: format!("{:016x}", t.key),
        dims: t.dims,
        origin: t.origin,
        spacing: t.spacing,
        patch: t.patch,
    }
}

const NO_IDENT: &str = "vec2 fs_ident(vec3 p) { return vec2(-1.0, 0.0); }\n";

fn bounds_of(e: &Expr) -> Option<[[f64; 3]; 2]> {
    let b = expr::bounds(e);
    if b.is_empty() {
        None
    } else {
        Some([b.lo, b.hi])
    }
}

/// GLSL for the body field plus its bounding box.
#[wasm_bindgen]
pub fn body_shader(doc_json: &str) -> Result<String, JsValue> {
    let doc = doc_from(doc_json)?;
    let e = expr::compile_body(&doc);
    let g = glsl::generate(&e);
    let (ident, leaves) = glsl::ident(&doc);
    to_json(&ShaderOut {
        source: g.source,
        bounds: bounds_of(&e),
        ident,
        leaves,
        meshes: g.meshes.iter().map(mesh_tex_out).collect(),
    })
}

/// GLSL for one shape as placed in the model (ancestor transforms and patterns applied),
/// ignoring its boolean role — used to draw a selected cutter as a ghost.
#[wasm_bindgen]
pub fn shape_shader(doc_json: &str, id: &str) -> Result<String, JsValue> {
    let doc = doc_from(doc_json)?;
    let e = expr::placed_shape(&doc, id).unwrap_or(Expr::Empty);
    let g = glsl::generate(&e);
    to_json(&ShaderOut {
        source: g.source,
        bounds: bounds_of(&e),
        ident: NO_IDENT.into(),
        leaves: vec![],
        meshes: g.meshes.iter().map(mesh_tex_out).collect(),
    })
}

/// GLSL `vec4 fs_overlay(vec3 p, float eps)` highlighting coloured groups of shape faces.
/// `items_json`: `[{"selectors": [{"shape", "face"}], "color": [r, g, b]}]`.
#[wasm_bindgen]
pub fn overlay_shader(doc_json: &str, items_json: &str) -> Result<String, JsValue> {
    let doc = doc_from(doc_json)?;
    let items: Vec<glsl::OverlayItem> = serde_json::from_str(items_json).map_err(js_err)?;
    Ok(glsl::overlay(&doc, &items))
}

/// Evaluate the body field at packed points `[x0, y0, z0, x1, …]`; returns `[d, gx, gy, gz, …]`.
#[wasm_bindgen]
pub fn eval_body(doc_json: &str, points: &[f64]) -> Result<Vec<f64>, JsValue> {
    let doc = doc_from(doc_json)?;
    let e = expr::compile_body(&doc);
    let mut out = Vec::with_capacity(points.len() / 3 * 4);
    for p in points.chunks_exact(3) {
        let d = expr::eval(&e, [p[0], p[1], p[2]]);
        out.extend_from_slice(&[d.v, d.g[0], d.g[1], d.g[2]]);
    }
    Ok(out)
}

#[derive(Serialize)]
struct Pick {
    t: f64,
    point: [f64; 3],
    normal: [f64; 3],
    shape: Option<String>,
    face: Option<String>,
    /// Surface patch of the hit on a mesh leaf (`mesh1.p3` selectors), `None` for
    /// primitive faces or when no leaf claims the point.
    patch: Option<u32>,
}

/// Cast a ray against the body; returns the hit and the shape/face it lies on, or `null`.
#[wasm_bindgen]
pub fn pick(
    doc_json: &str,
    ox: f64,
    oy: f64,
    oz: f64,
    dx: f64,
    dy: f64,
    dz: f64,
) -> Result<String, JsValue> {
    let doc = doc_from(doc_json)?;
    let e = expr::compile_body(&doc);
    let Some(t) = expr::raymarch(&e, [ox, oy, oz], [dx, dy, dz], 1e5) else {
        return Ok("null".into());
    };
    let dir = fieldsmith_field::dual::normalize([dx, dy, dz]);
    let point = fieldsmith_field::dual::add([ox, oy, oz], fieldsmith_field::dual::scale(dir, t));
    let d = expr::eval(&e, point);
    let surf = expr::identify_surface(&doc, point);
    let patch = surf
        .as_ref()
        .and_then(|(id, _, _)| expr::placed_shape(&doc, id))
        .and_then(|e| expr::eval_mesh_patch(&e, point))
        .map(|p| p as u32);
    to_json(&Pick {
        t,
        point,
        normal: fieldsmith_field::dual::normalize(d.g),
        shape: surf.as_ref().map(|s| s.0.clone()),
        face: surf.as_ref().map(|s| s.1.name().to_string()),
        patch,
    })
}

/// Import parameters from JS: units / up override the kind defaults, `voxel` requests a
/// spacing (mm), `src` is provenance, `ids` are the document's existing node ids so the
/// generated ids stay unique.
#[derive(serde::Deserialize)]
struct ImportOptions {
    units: Option<String>,
    up: Option<String>,
    voxel: Option<f64>,
    #[serde(default)]
    src: String,
    #[serde(default)]
    ids: Vec<String>,
}

/// Import a GLB / STL / OBJ / 3MF (`kind`: "glb" | "stl" | "obj" | "3mf") into documented
/// mesh nodes,
/// voxelising and hydrating the session registry. `opts_json`: `{units?, up?, voxel?,
/// src?, ids?}`. Returns `ImportOut` JSON — the nodes to splice into `doc.root` plus an
/// honest per-part report (tris, welded verts, patches, open edges, bounds, hash, cell
/// budget and timings).
#[wasm_bindgen(js_name = importMesh)]
pub fn import_mesh(bytes: &[u8], kind: &str, opts_json: &str) -> Result<String, JsValue> {
    let o: ImportOptions = serde_json::from_str(opts_json).map_err(js_err)?;
    let opts = fieldsmith_core::import::ImportOpts {
        units: o.units.as_deref(),
        up: o.up.as_deref(),
        voxel: o.voxel,
        src: &o.src,
        taken: &o.ids,
    };
    let out = fieldsmith_core::import::import_mesh(kind, bytes, &opts)?;
    to_json(&out)
}

/// One hydrated mesh grid ready for `Data3DTexture` upload: the distance sampler and, when
/// the import segmented patches, the parallel `u16` patch grid. Typed arrays are copies,
/// not wasm-memory views.
#[wasm_bindgen]
pub struct GridOut {
    key_hex: String,
    origin: [f64; 3],
    spacing: f64,
    dims: [usize; 3],
    patch: bool,
    dist: Float32Array,
    patch_data: Option<Uint16Array>,
}

#[wasm_bindgen]
impl GridOut {
    /// The registry key as 16 hex digits (`u_mesh{slot}` binding key).
    #[wasm_bindgen(js_name = keyHex)]
    pub fn key_hex(&self) -> String {
        self.key_hex.clone()
    }
    /// Grid node at `[ox, oy, oz]` (mm).
    #[wasm_bindgen(js_name = origin)]
    pub fn origin(&self) -> Float64Array {
        Float64Array::from(&self.origin[..])
    }
    /// Node spacing (mm).
    #[wasm_bindgen(js_name = spacing)]
    pub fn spacing(&self) -> f64 {
        self.spacing
    }
    /// Grid dims `[nx, ny, nz]`.
    #[wasm_bindgen(js_name = dims)]
    pub fn dims(&self) -> Uint32Array {
        Uint32Array::from(
            self.dims
                .iter()
                .map(|&d| d as u32)
                .collect::<Vec<u32>>()
                .as_slice(),
        )
    }
    /// Whether a parallel patch grid accompanies the distance grid.
    #[wasm_bindgen(js_name = patch)]
    pub fn patch(&self) -> bool {
        self.patch
    }
    /// Signed distances (f32, row-major `x` fastest — matches `Grid3::at` layout).
    #[wasm_bindgen(js_name = dist)]
    pub fn dist(&self) -> Float32Array {
        self.dist.clone()
    }
    /// Patch ids (u16, same layout; `u16::MAX` = unclaimed), `None` when unpatched.
    #[wasm_bindgen(js_name = patchData)]
    pub fn patch_data(&self) -> Option<Uint16Array> {
        self.patch_data.clone()
    }
}

/// Read a hydrated mesh grid by its 16-hex registry key (`keyHex` of a `MeshTexOut`,
/// or a mesh node's `hash`). Errors naming the reason when the key is malformed or the
/// registry has no entry — say, document loaded without its `.fsmz`.
#[wasm_bindgen(js_name = meshGrid)]
pub fn mesh_grid(key_hex: &str) -> Result<GridOut, JsValue> {
    let key = fieldsmith_field::meshfield::key_from_hex(key_hex);
    if key == 0 {
        return Err(js_err(format!("'{key_hex}' is not a mesh grid key")));
    }
    let f = fieldsmith_field::meshfield::lookup(key).ok_or_else(|| {
        js_err(format!(
            "mesh grid {key_hex} is not hydrated — open the .fsmz sidecar or re-import first"
        ))
    })?;
    Ok(GridOut {
        key_hex: format!("{key:016x}"),
        origin: f.dist.origin,
        spacing: f.dist.spacing,
        dims: f.dist.dims,
        patch: f.patch.is_some(),
        dist: Float32Array::from(f.dist.data.as_slice()),
        patch_data: f.patch.as_ref().map(|p| Uint16Array::from(p.as_slice())),
    })
}

/// Payload for `hydrate_mesh`: one mesh grid plus the import metadata that rides alongside
/// it, in the exact shape `mesh_grid` hands out (dist row-major `x` fastest; patch cells
/// `u16::MAX` where nothing is claimed).
#[derive(serde::Deserialize)]
struct HydrateMesh {
    key: String,
    origin: [f64; 3],
    spacing: f64,
    dims: [usize; 3],
    bbox: [f64; 6],
    tris: u32,
    patches: u32,
    open_edges: u32,
}

/// Hydrate the mesh-grid registry from arrays the JS side holds (a worker relaunching
/// after an edit, or the main thread after an import whose voxelisation ran elsewhere).
/// The arrays are the same encodings `mesh_grid` emits: dist little-endian f32, patch
/// little-endian u16 (`u16::MAX` = unclaimed). Returns the hydrated key as 16 hex digits.
#[wasm_bindgen(js_name = hydrateMesh)]
pub fn hydrate_mesh(
    spec_json: &str,
    dist: &[f32],
    patch: Option<Vec<u16>>,
) -> Result<String, JsValue> {
    let i: HydrateMesh = serde_json::from_str(spec_json).map_err(js_err)?;
    let key = fieldsmith_field::meshfield::key_from_hex(&i.key);
    if key == 0 {
        return Err(js_err(format!("'{}' is not a mesh grid key", i.key)));
    }
    let cells: usize = i.dims.iter().product();
    if dist.len() != cells {
        return Err(js_err(format!(
            "dist carries {} values, its dims [{}] expect {cells}",
            dist.len(),
            i.dims
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if let Some(p) = &patch {
        if p.len() != cells {
            return Err(js_err(format!(
                "patch grid carries {} ids, its dims expect {cells}",
                p.len()
            )));
        }
    }
    let field = fieldsmith_field::meshfield::MeshField {
        dist: std::sync::Arc::new(fieldsmith_field::lattice::Grid3 {
            origin: i.origin,
            spacing: i.spacing,
            dims: i.dims,
            data: dist.to_vec(),
        }),
        patch: patch.map(std::sync::Arc::new),
        meta: fieldsmith_field::meshfield::MeshMeta {
            bbox: fieldsmith_field::Aabb {
                lo: [i.bbox[0], i.bbox[1], i.bbox[2]],
                hi: [i.bbox[3], i.bbox[4], i.bbox[5]],
            },
            tris: i.tris,
            patches: i.patches,
            open_edges: i.open_edges,
        },
    };
    if !field.dist.data.iter().all(|v| v.is_finite()) {
        return Err(js_err("dist carries a non-finite value"));
    }
    fieldsmith_field::meshfield::hydrate(key, field);
    Ok(format!("{key:016x}"))
}

/// Whether a mesh grid key is hydrated (`false` for a malformed key).
#[wasm_bindgen(js_name = meshHydrated)]
pub fn mesh_hydrated(key_hex: &str) -> bool {
    let key = fieldsmith_field::meshfield::key_from_hex(key_hex);
    key != 0 && fieldsmith_field::meshfield::lookup(key).is_some()
}

/// Open a `.fsmz` sidecar: inflate the entries, parse `doc.fsm`, cross-check against
/// `manifest.json` and hydrate the mesh-grid registry. Returns `{doc, manifest}` JSON —
/// the document plus the parsed manifest (grid placements and CRCs).
#[wasm_bindgen(js_name = loadFsmz)]
pub fn load_fsmz(bytes: &[u8]) -> Result<String, JsValue> {
    to_json(&fieldsmith_core::fsmz::load(bytes)?)
}

/// Write the `.fsmz` sidecar for the document (`saveFsmz` takes the doc JSON exactly as
/// the editor holds it). Errors naming the hash when a mesh grid is not hydrated —
/// re-import the source or load its sidecar first.
#[wasm_bindgen(js_name = saveFsmz)]
pub fn save_fsmz(doc_json: &str) -> Result<Vec<u8>, JsValue> {
    let doc = doc_from(doc_json)?;
    fieldsmith_core::fsmz::save(&doc).map_err(js_err)
}

/// Faces a node exposes for region selection (JSON array of face names).
#[wasm_bindgen]
pub fn node_faces(doc_json: &str, id: &str) -> Result<String, JsValue> {
    let doc = doc_from(doc_json)?;
    let faces: Vec<&str> = doc
        .find(id)
        .and_then(expr::node_prim)
        .map(|p| {
            fieldsmith_field::prim::faces_of(&p)
                .iter()
                .map(|f| f.name())
                .collect()
        })
        .unwrap_or_default();
    to_json(&faces)
}

/// Body bounding box `[[lo], [hi]]` or `null`.
#[wasm_bindgen]
pub fn body_bounds(doc_json: &str) -> Result<String, JsValue> {
    let doc = doc_from(doc_json)?;
    to_json(&bounds_of(&expr::compile_body(&doc)))
}

/// Wall-clock milliseconds (durations only; the browser wall clock is the monotonic-enough
/// stand-in here).
fn clock() -> f64 {
    js_sys::Date::now()
}

#[derive(Serialize)]
struct AnalysisMeta {
    provenance: fieldsmith_core::Provenance,
    /// Per-case summaries: name, maxima and the solve report (big arrays skipped).
    cases: Vec<fieldsmith_core::CaseResult>,
    yield_strength: f64,
    poisson: f64,
    /// First mode frequencies when the caller asked for a modal run alongside the statics.
    modes: Option<fieldsmith_solve::ModalResult>,
}

/// One analysis, kept in wasm and queried through typed-array getters.
#[wasm_bindgen]
pub struct AnalysisOut {
    a: fieldsmith_core::Analysis,
}

/// Run the analysis pipeline. `h` targets an element size (mm), `dofs` a degree-of-freedom
/// count; pass one, leave the other `None`. A `solve modes=<n>` on the document adds the
/// first mode frequencies (`metaJson.modes`, in Hz, ascending), solved on the same mesh as
/// the static cases. Errors are user-fixable setup issues ("add a support: …").
#[wasm_bindgen(js_name = analyse)]
pub fn analyse(
    doc_json: &str,
    h: Option<f64>,
    dofs: Option<usize>,
) -> Result<AnalysisOut, JsValue> {
    let doc = doc_from(doc_json)?;
    let res = match (h, dofs) {
        (Some(h), _) => Resolution::Size(h),
        (_, Some(n)) => Resolution::Dofs(n),
        (None, None) => Resolution::Dofs(150_000),
    };
    let modes = doc.solve.modes;
    let a = core_analyse_full(&doc, res, clock, modes).map_err(js_err)?;
    Ok(AnalysisOut { a })
}

#[wasm_bindgen]
impl AnalysisOut {
    /// Provenance, per-case maxima and material constants as one JSON object. Region values
    /// (`surface_region`) index the document's own `regions` array, which the caller has.
    #[wasm_bindgen(js_name = metaJson)]
    pub fn meta_json(&self) -> Result<String, JsValue> {
        to_json(&AnalysisMeta {
            provenance: self.a.provenance.clone(),
            cases: self.a.cases.clone(),
            yield_strength: self.a.yield_strength,
            poisson: self.a.poisson,
            modes: self.a.modes.clone(),
        })
    }

    /// Nodal coordinates, 3 per node (mm).
    pub fn nodes(&self) -> Float64Array {
        let flat = self
            .a
            .mesh
            .nodes
            .iter()
            .flat_map(|p| p.iter())
            .copied()
            .collect::<Vec<f64>>();
        Float64Array::from(&flat[..])
    }

    /// Surface facets, 6 node ids each (corners, then midsides 01, 12, 20).
    #[wasm_bindgen(js_name = surfaceFacets)]
    pub fn surface_facets(&self) -> Uint32Array {
        let flat = self.a.surface.concat();
        Uint32Array::from(&flat[..])
    }

    /// Owning element index per surface facet.
    #[wasm_bindgen(js_name = surfaceOwner)]
    pub fn surface_owner(&self) -> Uint32Array {
        Uint32Array::from(&self.a.surface_owner[..])
    }

    /// Region index per surface facet (u32::MAX = no region).
    #[wasm_bindgen(js_name = surfaceRegion)]
    pub fn surface_region(&self) -> Uint32Array {
        Uint32Array::from(&self.a.surface_region[..])
    }

    /// Concatenated per-region facet sets, blocks in document order; block `ri` spans
    /// `regionFacetOffsets[ri] .. regionFacetOffsets[ri + 1]`. Unlike `surfaceRegion` a facet
    /// claimed by several regions appears in every claiming region's block.
    #[wasm_bindgen(js_name = regionFacetSets)]
    pub fn region_facet_sets(&self) -> Uint32Array {
        let flat: Vec<u32> = self.a.region_facet_sets.iter().flatten().copied().collect();
        Uint32Array::from(&flat[..])
    }

    /// Cumulative sizes of the `regionFacetSets` blocks, length = number of regions + 1.
    #[wasm_bindgen(js_name = regionFacetOffsets)]
    pub fn region_facet_offsets(&self) -> Uint32Array {
        let mut off = Vec::with_capacity(self.a.region_facet_sets.len() + 1);
        off.push(0);
        for set in &self.a.region_facet_sets {
            off.push(off[off.len() - 1] + set.len() as u32);
        }
        Uint32Array::from(&off[..])
    }

    /// Nodal displacement of one case, 3 per node (mm).
    pub fn u(&self, case: usize) -> Float64Array {
        Float64Array::from(&self.a.cases[case].u[..])
    }

    /// Nodal von Mises of one case (MPa).
    #[wasm_bindgen(js_name = vmNodal)]
    pub fn vm_nodal(&self, case: usize) -> Float64Array {
        Float64Array::from(&self.a.cases[case].von_mises[..])
    }

    /// Element-centroid von Mises of one case (MPa) — the readout statistic per V3.md.
    #[wasm_bindgen(js_name = vmElem)]
    pub fn vm_elem(&self, case: usize) -> Float64Array {
        Float64Array::from(&self.a.cases[case].vm_elem[..])
    }

    /// Max over cases of nodal von Mises (MPa).
    #[wasm_bindgen(js_name = vmEnvelope)]
    pub fn vm_envelope(&self) -> Float64Array {
        Float64Array::from(&self.a.envelope[..])
    }

    /// Max over cases of element-centroid von Mises (MPa).
    #[wasm_bindgen(js_name = vmElemEnvelope)]
    pub fn vm_elem_envelope(&self) -> Float64Array {
        Float64Array::from(&self.a.envelope_elem[..])
    }
}

#[derive(Serialize)]
struct CaseSummary {
    name: String,
    max_vm_elem: f64,
    max_displacement: f64,
}

#[derive(Serialize)]
struct FillInfo {
    family: String,
    mode: String,
    cell: f64,
    skin: f64,
    rho_min: f64,
    rho_max: f64,
    curve: f64,
    min_wall: f64,
    /// Achieved design density after wall clamps (min, max) — what the lattice actually is.
    rho_design: [f64; 2],
    sigma_ref: f64,
    mass_g: f64,
    solid_mass_g: f64,
    fill_ms: f64,
    solve_ms: f64,
}

#[derive(Serialize)]
struct GradedMeta {
    provenance: Provenance,
    /// The graded re-solve, one per load case, same order as the solid cases.
    cases: Vec<fieldsmith_core::CaseResult>,
    /// Solid-reference per-case maxima (before the fill was applied).
    solid_cases: Vec<CaseSummary>,
    fill: FillInfo,
    yield_strength: f64,
    poisson: f64,
}

/// A graded-fill analysis: the solid mesh plus the graded re-solve and fill result. The solid
/// reference numbers live in `metaJson.solidCases`; the getter surface exposes the graded
/// results (what the UI displays) and the fill itself.
#[wasm_bindgen]
pub struct GradedOut {
    base: Analysis,
    g: Graded,
    name: String,
    material: String,
}

/// Run the solid analysis and the graded fill re-solve in one call. `h`/`dofs` as in `analyse`.
#[wasm_bindgen(js_name = analyseGraded)]
pub fn analyse_graded(
    doc_json: &str,
    h: Option<f64>,
    dofs: Option<usize>,
) -> Result<GradedOut, JsValue> {
    let doc = doc_from(doc_json)?;
    let res = match (h, dofs) {
        (Some(h), _) => Resolution::Size(h),
        (_, Some(n)) => Resolution::Dofs(n),
        (None, None) => Resolution::Dofs(150_000),
    };
    let (a, g) = graded(&doc, res, clock).map_err(js_err)?;
    Ok(GradedOut {
        base: a,
        g,
        name: doc.name,
        material: doc.material,
    })
}

#[wasm_bindgen]
impl GradedOut {
    /// Provenance, graded per-case maxima, solid-reference maxima and the fill spec as one JSON
    /// object. `surfaceRegion` semantics are documented on `AnalysisOut`.
    #[wasm_bindgen(js_name = metaJson)]
    pub fn meta_json(&self) -> Result<String, JsValue> {
        to_json(&GradedMeta {
            provenance: self.base.provenance.clone(),
            cases: self.g.cases.clone(),
            solid_cases: self
                .base
                .cases
                .iter()
                .map(|c| CaseSummary {
                    name: c.name.clone(),
                    max_vm_elem: c.max_vm_elem,
                    max_displacement: c.max_displacement,
                })
                .collect(),
            fill: FillInfo {
                family: format!("{:?}", self.g.spec.family),
                mode: format!("{:?}", self.g.spec.mode),
                cell: self.g.spec.cell,
                skin: self.g.spec.skin,
                rho_min: self.g.spec.rho_min,
                rho_max: self.g.spec.rho_max,
                curve: self.g.spec.curve,
                min_wall: self.g.spec.min_wall,
                rho_design: self.g.rho_design,
                sigma_ref: self.g.sigma_ref,
                mass_g: self.g.mass_g,
                solid_mass_g: self.g.solid_mass_g,
                fill_ms: self.g.fill_ms,
                solve_ms: self.g.solve_ms,
            },
            yield_strength: self.base.yield_strength,
            poisson: self.base.poisson,
        })
    }

    /// Nodal coordinates of the solid mesh, 3 per node (mm).
    pub fn nodes(&self) -> Float64Array {
        let flat = self
            .base
            .mesh
            .nodes
            .iter()
            .flat_map(|p| p.iter())
            .copied()
            .collect::<Vec<f64>>();
        Float64Array::from(&flat[..])
    }

    /// Surface facets, 6 node ids each (corners, then midsides 01, 12, 20).
    #[wasm_bindgen(js_name = surfaceFacets)]
    pub fn surface_facets(&self) -> Uint32Array {
        Uint32Array::from(&self.base.surface.concat()[..])
    }

    /// Owning element index per surface facet.
    #[wasm_bindgen(js_name = surfaceOwner)]
    pub fn surface_owner(&self) -> Uint32Array {
        Uint32Array::from(&self.base.surface_owner[..])
    }

    /// Region index per surface facet (u32::MAX = no region).
    #[wasm_bindgen(js_name = surfaceRegion)]
    pub fn surface_region(&self) -> Uint32Array {
        Uint32Array::from(&self.base.surface_region[..])
    }

    /// Concatenated per-region facet sets of the solid base mesh, blocks in document order;
    /// see `AnalysisOut::region_facet_sets`.
    #[wasm_bindgen(js_name = regionFacetSets)]
    pub fn region_facet_sets(&self) -> Uint32Array {
        let flat: Vec<u32> = self
            .base
            .region_facet_sets
            .iter()
            .flatten()
            .copied()
            .collect();
        Uint32Array::from(&flat[..])
    }

    /// Cumulative sizes of the `regionFacetSets` blocks, length = number of regions + 1.
    #[wasm_bindgen(js_name = regionFacetOffsets)]
    pub fn region_facet_offsets(&self) -> Uint32Array {
        let mut off = Vec::with_capacity(self.base.region_facet_sets.len() + 1);
        off.push(0);
        for set in &self.base.region_facet_sets {
            off.push(off[off.len() - 1] + set.len() as u32);
        }
        Uint32Array::from(&off[..])
    }

    /// Graded nodal displacement of one case, 3 per node (mm).
    pub fn u(&self, case: usize) -> Float64Array {
        Float64Array::from(&self.g.cases[case].u[..])
    }

    /// Graded nodal von Mises of one case (MPa).
    #[wasm_bindgen(js_name = vmNodal)]
    pub fn vm_nodal(&self, case: usize) -> Float64Array {
        Float64Array::from(&self.g.cases[case].von_mises[..])
    }

    /// Graded element-centroid von Mises of one case (MPa) — the readout statistic per V3.md.
    #[wasm_bindgen(js_name = vmElem)]
    pub fn vm_elem(&self, case: usize) -> Float64Array {
        Float64Array::from(&self.g.cases[case].vm_elem[..])
    }

    /// Graded max over cases of nodal von Mises (MPa).
    #[wasm_bindgen(js_name = vmEnvelope)]
    pub fn vm_envelope(&self) -> Float64Array {
        Float64Array::from(&self.g.envelope[..])
    }

    /// Graded max over cases of element-centroid von Mises (MPa).
    #[wasm_bindgen(js_name = vmElemEnvelope)]
    pub fn vm_elem_envelope(&self) -> Float64Array {
        Float64Array::from(&self.g.envelope_elem[..])
    }

    /// Per-element class: 0 solid (skin, protected), 1 lattice, 2 void.
    pub fn class(&self) -> Uint8Array {
        Uint8Array::from(&self.g.class[..])
    }

    /// Per-element material fraction of the fill (1 solid, 0 void; the modulus input).
    #[wasm_bindgen(js_name = rhoElem)]
    pub fn rho_elem(&self) -> Float64Array {
        Float64Array::from(&self.g.rho_elem[..])
    }

    /// Graded fill parameter grid spec `[ox, oy, oz, spacing]` (mm).
    #[wasm_bindgen(js_name = fillParamSpec)]
    pub fn fill_param_spec(&self) -> Float64Array {
        let o = &self.g.fill.param.origin;
        Float64Array::from(&[o[0], o[1], o[2], self.g.fill.param.spacing][..])
    }

    /// Graded fill parameter grid dims `[nx, ny, nz]`.
    #[wasm_bindgen(js_name = fillParamDims)]
    pub fn fill_param_dims(&self) -> Uint32Array {
        let d = self.g.fill.param.dims.map(|x| x as u32);
        Uint32Array::from(&d[..])
    }

    /// Graded density values on the parameter grid, row-major with x fastest (relative density).
    #[wasm_bindgen(js_name = fillParamData)]
    pub fn fill_param_data(&self) -> Float32Array {
        Float32Array::from(&self.g.fill.param.data[..])
    }

    /// Export the stored graded fill at mesh spacing `h` (mm): meshes the filled field
    /// without re-solving. `metaJson` reports what the mesh is relative to the field
    /// (`what: "graded fill"`), including the mesh/field volume delta and any defects.
    pub fn export(&self, h: f64) -> Result<MeshOut, JsValue> {
        if !h.is_finite() || h <= 0.0 {
            return Err(js_err("mesh spacing h must be positive and finite (mm)"));
        }
        let field = Expr::Filled(Box::new(self.g.fill.clone()));
        export_field(&field, &self.name, &self.material, h, "graded fill", 0.0)
    }
}

#[derive(Serialize)]
struct ExportMeta {
    /// What was meshed: "solid" or "graded fill".
    what: &'static str,
    h: f64,
    verts: usize,
    tris: usize,
    field_volume: f64,
    field_err: f64,
    mesh_volume: f64,
    delta_pct: f64,
    open_edges: usize,
    non_manifold_edges: usize,
    orientation_defects: usize,
    degenerate_triangles: usize,
    self_intersections: usize,
    build_ms: f64,
    mesh_ms: f64,
    ref_ms: f64,
    scan_ms: f64,
}

/// An export mesh: typed-array access to the triangles, a health report on `metaJson`, byte
/// writers for the download formats, and plane slicing for the layer preview.
#[wasm_bindgen]
pub struct MeshOut {
    m: TriMesh,
    name: String,
    material: String,
    meta: ExportMeta,
}

#[wasm_bindgen]
impl MeshOut {
    #[wasm_bindgen(js_name = metaJson)]
    pub fn meta_json(&self) -> Result<String, JsValue> {
        to_json(&self.meta)
    }

    /// Vertices, 3 per vertex (mm).
    pub fn verts(&self) -> Float64Array {
        let flat = self
            .m
            .verts
            .iter()
            .flat_map(|p| p.iter())
            .copied()
            .collect::<Vec<f64>>();
        Float64Array::from(&flat[..])
    }

    /// Triangles, 3 vertex ids each.
    pub fn tris(&self) -> Uint32Array {
        let flat = self
            .m
            .tris
            .iter()
            .flat_map(|t| t.iter())
            .copied()
            .collect::<Vec<u32>>();
        Uint32Array::from(&flat[..])
    }

    /// Binary STL bytes (millimetre units).
    #[wasm_bindgen(js_name = stlBytes)]
    pub fn stl_bytes(&self) -> Uint8Array {
        Uint8Array::from(self.m.write_stl().as_slice())
    }

    /// 3MF package bytes: the part name and material recorded, vertices translated into the
    /// positive octant (3MF requires it).
    #[wasm_bindgen(js_name = threeMfBytes)]
    pub fn three_mf_bytes(&self) -> Uint8Array {
        Uint8Array::from(self.m.write_3mf(&self.name, &self.material).as_slice())
    }

    /// Segments of the plane z through the mesh, packed `[x0, y0, x1, y1, …]` (mm): the
    /// contours of the exact exported mesh, for the layer preview.
    #[wasm_bindgen(js_name = sliceAt)]
    pub fn slice_at(&self, z: f64) -> Float64Array {
        Float64Array::from(&self.m.slice_at(z)[..])
    }
}

/// The export surface plus its measured health report. `build_ms` is time already spent
/// preparing `field` (the graded solve) so callers can show it separately.
fn export_field(
    field: &Expr,
    name: &str,
    material: &str,
    h: f64,
    what: &'static str,
    build_ms: f64,
) -> Result<MeshOut, JsValue> {
    let b = expr::bounds(field);
    if b.is_empty() {
        return Err(js_err("document has no solid"));
    }
    let t0 = clock();
    // Adaptive dual contouring: finer only near the surface, which is what makes GOAL's
    // 0.1 mm export budget reachable. The defect classes it carries on the graded fill
    // (flat wall tiles overlapping within multi-sheet cells, corner-snap degeneracies) are
    // the mesher's own version of the same fill-surface structure V8 classifies for the
    // uniform path; the measured class counts travel in `metaJson` below.
    let m = surface_adaptive(
        |p| {
            let d = expr::eval(field, p);
            Sample { v: d.v, g: d.g }
        },
        [b.lo[0] - h, b.lo[1] - h, b.lo[2] - h],
        [b.hi[0] + h, b.hi[1] + h, b.hi[2] + h],
        h,
        &AdaptiveOpts::default(),
    );
    let mesh_ms = clock() - t0;
    #[cfg(target_arch = "wasm32")]
    let t0 = clock();
    // Reference pair at the MESH spacing, not a coarser auto spacing: on TPMS fill fields
    // the interval's error is plate-thickness-quantisation and it grows with h, so a coarser
    // reference understates the volume and its Richardson band misses the truth (bracket
    // fill at h ≈ 0.35: −1.3 % off the converged integral while reporting ± 0.8 %). The
    // mesh-h pair keeps the reported band honest for the same ~0.6 s cost (see the
    // v8_ref_probe ladder: at h = 0.25 the pair's band is ± 0.76 % around −0.65 %).
    let v_field = fieldsmith_field::volume::volume(field, h);
    let ref_ms = clock() - t0;
    let v_mesh = m.volume();
    let delta = if v_field.volume > 0.0 {
        (v_mesh - v_field.volume) / v_field.volume * 100.0
    } else {
        f64::NAN
    };
    let t0 = clock();
    let d = m.defects();
    let scan_ms = clock() - t0;
    let meta = ExportMeta {
        what,
        h,
        verts: m.verts.len(),
        tris: m.tris.len(),
        field_volume: v_field.volume,
        field_err: v_field.error,
        mesh_volume: v_mesh,
        delta_pct: delta,
        open_edges: d.open_edges,
        non_manifold_edges: d.non_manifold_edges,
        orientation_defects: d.orientation_defects,
        degenerate_triangles: d.degenerate_triangles,
        self_intersections: d.self_intersections,
        build_ms,
        mesh_ms,
        ref_ms,
        scan_ms,
    };
    Ok(MeshOut {
        m,
        name: name.to_string(),
        material: material.to_string(),
        meta,
    })
}

/// Export the solid body at mesh spacing `h` (mm). The reported field volume comes from the
/// same field the mesh was cut from, so the delta in `metaJson` is the mesher's
/// representation error, not a modelling difference.
#[wasm_bindgen(js_name = exportSolid)]
pub fn export_solid(doc_json: &str, h: f64) -> Result<MeshOut, JsValue> {
    if !h.is_finite() || h <= 0.0 {
        return Err(js_err("mesh spacing h must be positive and finite (mm)"));
    }
    let doc = doc_from(doc_json)?;
    let e = expr::compile_body(&doc);
    export_field(&e, &doc.name, &doc.material, h, "solid", 0.0)
}

/// Export the graded fill at mesh spacing `h` (mm), building the graded fill first (stress
/// solve included, ~5 s at the browser default). Reuse an existing `GradedOut` through its
/// `export` method when one is already solved.
#[wasm_bindgen(js_name = exportGraded)]
pub fn export_graded(doc_json: &str, h: f64) -> Result<MeshOut, JsValue> {
    if !h.is_finite() || h <= 0.0 {
        return Err(js_err("mesh spacing h must be positive and finite (mm)"));
    }
    let doc = doc_from(doc_json)?;
    let t0 = clock();
    let (_, g) = graded(&doc, Resolution::Dofs(150_000), clock).map_err(js_err)?;
    let field = expr::Expr::Filled(Box::new(g.fill));
    export_field(
        &field,
        &doc.name,
        &doc.material,
        h,
        "graded fill",
        clock() - t0,
    )
}
