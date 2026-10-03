//! WebAssembly bindings. Documents cross the boundary as JSON strings (the serde form of
//! [`fieldsmith_doc::Document`]); bulk numeric data crosses as typed arrays.

use fieldsmith_doc::{fsm, materials, Document};
use fieldsmith_field::{expr, glsl, Expr};
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
