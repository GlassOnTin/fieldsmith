//! WebAssembly bindings. Documents cross the boundary as JSON strings (the serde form of
//! [`fieldsmith_doc::Document`]); bulk numeric data crosses as typed arrays.

use fieldsmith_core::{analyse as core_analyse, Resolution};
use fieldsmith_doc::{fsm, materials, Document};
use fieldsmith_field::{expr, glsl, Expr};
use js_sys::{Float64Array, Uint32Array};
use serde::Serialize;
use wasm_bindgen::prelude::*;

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
    to_json(&Pick {
        t,
        point,
        normal: fieldsmith_field::dual::normalize(d.g),
        shape: surf.as_ref().map(|s| s.0.clone()),
        face: surf.as_ref().map(|s| s.1.name().to_string()),
    })
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
}

/// One analysis, kept in wasm and queried through typed-array getters.
#[wasm_bindgen]
pub struct AnalysisOut {
    a: fieldsmith_core::Analysis,
}

/// Run the analysis pipeline. `h` targets an element size (mm), `dofs` a degree-of-freedom
/// count; pass one, leave the other `None`. Errors are user-fixable setup issues ("add a
/// support: …").
#[wasm_bindgen(js_name = analyse)]
pub fn analyse(doc_json: &str, h: Option<f64>, dofs: Option<usize>) -> Result<AnalysisOut, JsValue> {
    let doc = doc_from(doc_json)?;
    let res = match (h, dofs) {
        (Some(h), _) => Resolution::Size(h),
        (_, Some(n)) => Resolution::Dofs(n),
        (None, None) => Resolution::Dofs(150_000),
    };
    let a = core_analyse(&doc, res, clock).map_err(js_err)?;
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
        })
    }

    /// Nodal coordinates, 3 per node (mm).
    pub fn nodes(&self) -> Float64Array {
        let flat = self.a.mesh.nodes.iter().flat_map(|p| p.iter()).copied().collect::<Vec<f64>>();
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
