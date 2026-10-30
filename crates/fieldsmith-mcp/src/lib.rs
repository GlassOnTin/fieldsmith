//! The MCP stdio server core (GOAL v0.4 section C, slice 2). Protocol logic is a pure
//! in-process function pair — [`Server::handle`] turns one request line into at most
//! one response line — so the verify tests drive the exact code path the binary runs.
//!
//! Tools are thin over the shared crates: documents open through [`load_doc`], edits
//! go through the typed document model, `solve` builds the same `{apiv, doc, question}`
//! request the one-shot verb accepts and calls [`respond`], and `export` runs the
//! [`export`] core. Same-result is then by construction, and V22 pins it.
//!
//! Protocol rules implemented here (MCP stdio, newline-delimited JSON-RPC 2.0):
//! `initialize`, `ping`, `tools/list`, `tools/call`; a notification carries no `id`,
//! so it produces no response; malformed JSON is `-32700` with a null id; an unknown
//! method is `-32601`. A tool that cannot do its work returns a result with
//! `isError: true` and a text message — tool errors are answers, not protocol errors.

use std::sync::OnceLock;

use fieldsmith_api::schema::{QuestionSpec, APIV};
use fieldsmith_api::{b64, export, load_doc, missing_grids, question, respond};
use fieldsmith_core::fsmz;
use fieldsmith_doc::{
    fsm, param, Document, Family, FillMode, Let, Load, LoadKind, NodeKind, Region, Selector,
    Support, Temperature,
};
use serde_json::{json, Value};

pub const PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "fieldsmith-mcp";

pub struct Server {
    doc: Option<Document>,
}

impl Server {
    pub fn new() -> Self {
        Self { doc: None }
    }

    /// One request in, at most one response out. Returns `None` for notifications
    /// (messages without an `id`) and for blank lines.
    pub fn handle(&mut self, line: &str) -> Option<String> {
        if line.trim().is_empty() {
            return None;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => return Some(err_reply(Value::Null, -32700, "parse error")),
        };
        let obj = match msg.as_object() {
            Some(o) => o,
            None => return Some(err_reply(Value::Null, -32600, "invalid request")),
        };
        let id = obj.get("id").cloned();
        let method = match obj.get("method").and_then(Value::as_str) {
            Some(m) => m,
            None => {
                return Some(err_reply(
                    id.clone().unwrap_or(Value::Null),
                    -32600,
                    "invalid request",
                ))
            }
        };
        // A notification (no id) never gets a response — including an unknown one.
        let id = id?;

        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION")},
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools_catalog()})),
            "tools/call" => self.tools_call(obj.get("params")),
            _ => Err(json_err(-32601, "method not found")),
        };
        Some(match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string(),
            Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}).to_string(),
        })
    }

    fn tools_call(&mut self, params: Option<&Value>) -> Result<Value, Value> {
        let params = params
            .and_then(Value::as_object)
            .ok_or_else(|| json_err(-32602, "tools/call needs params"))?;
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| json_err(-32602, "tools/call needs a tool name"))?;
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        match self.tool(name, args) {
            Ok(v) => Ok(json!({"content": [{"type": "text", "text": v.to_string()}]})),
            Err(message) => Ok(json!({
                "content": [{"type": "text", "text": message}],
                "isError": true,
            })),
        }
    }

    /// One tool. `Ok` is the tool's facts/artifact JSON (the tool result text);
    /// `Err` is the `isError: true` message string.
    fn tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        match name {
            "open" => self.open(args),
            "inspect" => self.inspect(args),
            "add_region" => self.add_region(args),
            "add_support" => self.add_support(args),
            "add_load" => self.add_load(args),
            "add_temperature" => self.add_temperature(args),
            "set_material" => self.set_material(args),
            "set_let" => self.set_let(args),
            "set_fill" => self.set_fill(args),
            "set_build" => self.set_build(args),
            "doc_text" => self.doc_text(args),
            "solve" => self.solve(args),
            "export" => self.export(args),
            _ => Err(format!("unknown tool: {name}")),
        }
    }

    fn doc(&self) -> Result<&Document, String> {
        match &self.doc {
            Some(d) => Ok(d),
            None => Err(
                "no document open — call `open` with doc (.fsm text) or docz \
                         (base64 .fsmz) first"
                    .into(),
            ),
        }
    }

    /// Exactly one of doc/docz, through the shared loader; the open is the session's
    /// state. A `doc` text import carries no grids, so `missing_grids` names any mesh
    /// the session cannot yet evaluate.
    fn open(&mut self, args: Value) -> Result<Value, String> {
        let doc = args.get("doc").and_then(Value::as_str);
        let docz = args.get("docz").and_then(Value::as_str);
        if doc.is_none() && docz.is_none() {
            return Err("open: pass exactly one of doc (.fsm text) or docz (base64 .fsmz)".into());
        }
        let d = load_doc(doc, docz)?;
        let summary = doc_summary(&d);
        self.doc = Some(d);
        Ok(summary)
    }

    fn inspect(&mut self, _args: Value) -> Result<Value, String> {
        let d = self.doc()?;
        let fill = json!(d.fill);
        Ok(json!({
            "name": d.name,
            "material": d.material,
            "lets": d.lets,
            "custom_materials": d.custom_materials,
            "fill": fill,
            "solve": d.solve,
            "regions": d.regions,
            "supports": d.supports,
            "loads": d.loads,
            "temperatures": d.temperatures,
            "missing_grids": missing_grids(d),
            "problems": d.validate(),
        }))
    }

    fn add_region(&mut self, args: Value) -> Result<Value, String> {
        let selectors: Vec<Selector> = field("selectors", &args)?;
        self.add_entity("region", args, |d, id| {
            d.regions.push(Region { id, selectors });
            Ok(())
        })
    }

    fn add_support(&mut self, args: Value) -> Result<Value, String> {
        let region = text_field("region", &args)?;
        let fixed: [bool; 3] = field("fixed", &args)?;
        self.add_entity("support", args, |d, id| {
            d.supports.push(Support { id, region, fixed });
            Ok(())
        })
    }

    fn add_load(&mut self, args: Value) -> Result<Value, String> {
        let region = text_field("region", &args)?;
        let case = case_or_default(&args, "Load 1");
        let kind: LoadKind = field("kind", &args)?;
        self.add_entity("load", args, |d, id| {
            d.loads.push(Load {
                id,
                region,
                case,
                kind,
            });
            Ok(())
        })
    }

    fn add_temperature(&mut self, args: Value) -> Result<Value, String> {
        let region = text_field("region", &args)?;
        let celsius = number_field("celsius", &args)?;
        let case = case_or_default(&args, "Thermal 1");
        self.add_entity("temperature", args, |d, id| {
            d.temperatures.push(Temperature {
                id,
                region,
                celsius,
                case,
            });
            Ok(())
        })
    }

    /// The add_* body: optional `id` (else a fresh id with the tool's prefix), push the
    /// entity, report it with the document's current problem list — the same `validate`
    /// the `check` verb prints.
    fn add_entity<F>(&mut self, prefix: &str, args: Value, build: F) -> Result<Value, String>
    where
        F: FnOnce(&mut Document, String) -> Result<(), String>,
    {
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let id = match args.get("id") {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            _ => d.fresh_id(prefix),
        };
        build(d, id.clone())?;
        let problems = d.validate();
        Ok(json!({"added": id, "problems": problems}))
    }

    fn set_material(&mut self, args: Value) -> Result<Value, String> {
        let node = text_field("node", &args)?;
        let material = text_field("material", &args)?;
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        if d.set_node_material(&node, &material) {
            Ok(json!({"node": node, "material": material}))
        } else {
            Err(format!("set_material: no shape named {node:?}"))
        }
    }

    /// Update an existing `let` variable (or dotted parameter address `<id>.<attr>[.<n>]`),
    /// or define a new `let` variable, and re-evaluate all dependent consumer expressions.
    fn set_let(&mut self, args: Value) -> Result<Value, String> {
        let name = text_field("name", &args)?;
        let value = number_field("value", &args)?;
        if !value.is_finite() {
            return Err(format!("set_let: value {value} is not finite"));
        }
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        if name.contains('.') {
            let addr = param::ParamAddr::parse(&name).map_err(|e| format!("set_let: {e}"))?;
            param::apply(d, &addr, value).map_err(|e| format!("set_let: {e}"))?;
            let problems = d.validate();
            return Ok(json!({"name": name, "value": value, "problems": problems}));
        }
        if let Some(slot) = d.lets.iter().position(|l| l.name == name) {
            let addr = param::ParamAddr::parse(&name).map_err(|e| format!("set_let: {e}"))?;
            param::apply(d, &addr, value).map_err(|e| format!("set_let: {e}"))?;
            d.lets[slot].expr = format!("{value}");
            let problems = d.validate();
            Ok(json!({"name": name, "value": value, "lets": d.lets, "problems": problems}))
        } else {
            if !fsm::is_ident(&name) {
                return Err(format!("set_let: {name:?} is not a valid let identifier"));
            }
            d.lets.push(Let {
                name: name.clone(),
                expr: format!("{value}"),
                value,
            });
            let problems = d.validate();
            Ok(json!({"name": name, "value": value, "lets": d.lets, "problems": problems}))
        }
    }

    /// Configure or update the document's `fill` specification (TPMS family, mode, cell,
    /// skin, `skin_max`, density range, curve, min_wall, protected shapes, margin, case,
    /// or lattice rotation), or clear it when `"clear": true` is passed.
    fn set_fill(&mut self, args: Value) -> Result<Value, String> {
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        if flag(&args, "clear") {
            d.fill = None;
            let problems = d.validate();
            return Ok(json!({"fill": Value::Null, "problems": problems}));
        }
        let mut f = d.fill.clone().unwrap_or_default();
        if let Some(fam) = args.get("family").and_then(Value::as_str) {
            f.family = match fam {
                "gyroid" => Family::Gyroid,
                "primitive" => Family::Primitive,
                "diamond" => Family::Diamond,
                other => return Err(format!("set_fill: unknown lattice family {other:?}")),
            };
        }
        if let Some(mode) = args.get("mode").and_then(Value::as_str) {
            f.mode = match mode {
                "sheet" => FillMode::Sheet,
                "network" => FillMode::Network,
                other => return Err(format!("set_fill: unknown fill mode {other:?}")),
            };
        }
        if let Some(v) = args.get("cell").and_then(Value::as_f64) {
            f.cell = v;
        }
        if let Some(v) = args.get("skin").and_then(Value::as_f64) {
            f.skin = v;
        }
        if let Some(sm) = args.get("skin_max") {
            if sm.is_null() {
                f.skin_max = None;
            } else if let Some(v) = sm.as_f64() {
                f.skin_max = if v <= f.skin { None } else { Some(v) };
            } else {
                return Err("set_fill: skin_max must be a number or null".into());
            }
        }
        if let Some(v) = args.get("rho_min").and_then(Value::as_f64) {
            f.rho_min = v;
        }
        if let Some(v) = args.get("rho_max").and_then(Value::as_f64) {
            f.rho_max = v;
        }
        if let Some(v) = args.get("curve").and_then(Value::as_f64) {
            f.curve = v;
        }
        if let Some(v) = args.get("min_wall").and_then(Value::as_f64) {
            f.min_wall = v;
        }
        if let Some(v) = args.get("margin").and_then(Value::as_f64) {
            f.margin = v;
        }
        if let Some(p) = args.get("protect") {
            f.protect = serde_json::from_value(p.clone())
                .map_err(|e| format!("set_fill: protect: {e}"))?;
        }
        if let Some(c) = args.get("case") {
            f.case = if c.is_null() {
                None
            } else {
                Some(
                    c.as_str()
                        .ok_or_else(|| "set_fill: case must be a string or null".to_string())?
                        .to_string(),
                )
            };
        }
        if let Some(rot) = args.get("rotate") {
            f.rotate = serde_json::from_value(rot.clone())
                .map_err(|e| format!("set_fill: rotate: {e}"))?;
        }
        d.fill = Some(f);
        let problems = d.validate();
        Ok(json!({"fill": d.fill, "problems": problems}))
    }

    /// Set or clear the document's build orientation `solve build=(theta_deg, phi_deg)`
    /// (and optional `size`).
    fn set_build(&mut self, args: Value) -> Result<Value, String> {
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        if flag(&args, "clear") {
            d.solve.build = None;
        } else if let Some(ang) = args.get("angles") {
            let pair: [f64; 2] = serde_json::from_value(ang.clone())
                .map_err(|e| format!("set_build: angles: {e}"))?;
            d.solve.build = Some(pair);
        } else {
            let theta = number_field("theta", &args)?;
            let phi = number_field("phi", &args)?;
            d.solve.build = Some([theta, phi]);
        }
        if let Some(sz) = args.get("size") {
            d.solve.size = if sz.is_null() {
                None
            } else {
                Some(
                    sz.as_f64()
                        .ok_or_else(|| "set_build: size must be a number or null".to_string())?,
                )
            };
        }
        let problems = d.validate();
        Ok(json!({"solve": d.solve, "problems": problems}))
    }

    fn doc_text(&mut self, _args: Value) -> Result<Value, String> {
        let d = self.doc()?;
        Ok(json!({"text": fsm::print(d)}))
    }

    /// The API question, unchanged: the tool arguments ARE a `QuestionSpec` (its serde
    /// shape, `kind`-tagged), so parsing the arguments with it is the same discipline the
    /// request parser applies. The session's document travels as `.fsm` text (a print of
    /// the current model, edits included), or as a rebuilt `.fsmz` when the document has
    /// mesh nodes and their grids are in this process — the form a worker job would carry.
    /// With mesh nodes but grids missing, the text form goes to [`respond`] anyway so the
    /// refusal is the engine's own contract sentence, identical to what a same-process
    /// API request gets.
    fn solve(&mut self, args: Value) -> Result<Value, String> {
        let spec: QuestionSpec = serde_json::from_value(args).map_err(|e| format!("solve: {e}"))?;
        let d = self.doc()?;
        let text = fsm::print(d);
        let has_mesh = has_mesh_nodes(d);
        let req = if has_mesh && missing_grids(d).is_empty() {
            let bytes = fsmz::save(d)?;
            json!({"apiv": APIV, "docz": b64::encode(&bytes), "question": spec})
        } else {
            json!({"apiv": APIV, "doc": text, "question": spec})
        };
        let reply = respond(&req.to_string());
        // A processing error (the one-shot's exit 2) is a failed tool call; ok and
        // refusal envelopes are answers. The envelope travels as parsed JSON — the tool
        // result text must BE the envelope, not a quoted copy of it.
        if reply.is_error {
            Err(reply.body)
        } else {
            serde_json::from_str(&reply.body)
                .map_err(|e| format!("the engine envelope failed to re-parse: {e}"))
        }
    }

    /// The export core: facts (including the display timings) and — when requested —
    /// the artifact bytes base64-encoded in a second text. No file is written; the
    /// client decides.
    fn export(&mut self, args: Value) -> Result<Value, String> {
        let d = self.doc()?;
        let h = match args.get("h") {
            Some(v) => v
                .as_f64()
                .ok_or_else(|| "export: h must be a number".to_string())?,
            None => 0.25,
        };
        if flag(&args, "slices") {
            let layer_h = match args.get("layer_h") {
                Some(v) => v
                    .as_f64()
                    .ok_or_else(|| "export: layer_h must be a number".to_string())?,
                None => 0.2,
            };
            let sopts = export::ExportSlicesOpts {
                h,
                layer_h,
                solid_only: flag(&args, "solid_only"),
                mf: flag(&args, "mf"),
            };
            let sx = export::export_slices(d, &sopts, question::clock)?;
            let mut facts = json!({
                "name": d.name,
                "slices": true,
                "h": sx.h,
                "layer_h": sx.layer_h,
                "layers": sx.layers,
                "polygons": sx.polygons,
                "open_chains": sx.open_chains,
                "stack_volume": sx.stack_volume,
                "field_volume": sx.reference.volume,
                "field_volume_error": sx.reference.error,
                "delta_percent": sx.delta_percent,
                "slice_ms": sx.slice_ms,
                "ref_ms": sx.ref_ms,
                "total_ms": sx.ms,
            });
            if let Some(g) = sx.graded {
                facts["graded"] =
                    json!({"mass_g": g.mass_g, "solid_mass_g": g.solid_mass_g, "ms": g.ms});
            }
            return if sx.mf.is_some() {
                Ok(json!({
                    "facts": facts,
                    "mf_b64": sx.mf.map(|b| b64::encode(&b)),
                }))
            } else {
                Ok(facts)
            };
        }
        let opts = export::ExportOpts {
            h,
            adaptive: flag(&args, "adaptive"),
            solid_only: flag(&args, "solid_only"),
            stl: flag(&args, "stl"),
            mf: flag(&args, "mf"),
        };
        let ex = export::export(d, &opts, question::clock)?;
        let mut facts = json!({
            "name": d.name,
            "h": ex.h,
            "adaptive": ex.adaptive,
            "verts": ex.verts,
            "tris": ex.tris,
            "mesh_volume": ex.mesh_volume,
            "field_volume": ex.reference.volume,
            "field_volume_error": ex.reference.error,
            "delta_percent": ex.delta_percent,
            "defects": {
                "open_edges": ex.defects.open_edges,
                "non_manifold_edges": ex.defects.non_manifold_edges,
                "orientation_defects": ex.defects.orientation_defects,
                "degenerate_triangles": ex.defects.degenerate_triangles,
                "self_intersections": ex.defects.self_intersections,
            },
            "mesh_ms": ex.mesh_ms,
            "ref_ms": ex.ref_ms,
            "scan_ms": ex.scan_ms,
            "total_ms": ex.ms,
        });
        if let Some(g) = ex.graded {
            facts["graded"] =
                json!({"mass_g": g.mass_g, "solid_mass_g": g.solid_mass_g, "ms": g.ms});
        }
        if !ex.parts.is_empty() {
            facts["volumes"] = json!(ex.parts.iter().map(|p| json!({
                "id": p.id, "material": p.material, "verts": p.verts, "tris": p.tris,
                "field_volume": p.volume.volume, "field_volume_error": p.volume.error, "ms": p.ms,
            })).collect::<Vec<_>>());
        }
        if ex.stl.is_some() || ex.mf.is_some() {
            Ok(json!({
                "facts": facts,
                "stl_b64": ex.stl.map(|b| b64::encode(&b)),
                "mf_b64": ex.mf.map(|b| b64::encode(&b)),
            }))
        } else {
            Ok(facts)
        }
    }
}

fn json_err(code: i32, message: &str) -> Value {
    json!({"code": code, "message": message})
}

fn err_reply(id: Value, code: i32, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": json_err(code, message)}).to_string()
}

fn flag(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn has_mesh_nodes(d: &Document) -> bool {
    fn rec(nodes: &[fieldsmith_doc::Node]) -> bool {
        nodes.iter().any(|n| {
            matches!(n.kind, NodeKind::Mesh { .. })
                || match &n.kind {
                    NodeKind::Group { children, .. } => rec(children),
                    _ => false,
                }
        })
    }
    rec(&d.root)
}

fn doc_summary(d: &Document) -> Value {
    json!({
        "name": d.name,
        "shapes": d.node_ids().len(),
        "regions": d.regions.len(),
        "supports": d.supports.len(),
        "loads": d.loads.len(),
        "temperatures": d.temperatures.len(),
        "missing_grids": missing_grids(d),
        "problems": d.validate(),
    })
}

const NO_DOC: &str =
    "no document open — call `open` with doc (.fsm text) or docz (base64 .fsmz) first";

fn text_field(key: &str, args: &Value) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{key}: expected a string"))
}

fn number_field(key: &str, args: &Value) -> Result<f64, String> {
    args.get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("{key}: expected a number"))
}

/// A required field deserialized into its typed form — the same serde the document
/// parser applies to the equivalent .fsm text lines.
fn field<T: serde::de::DeserializeOwned>(key: &str, args: &Value) -> Result<T, String> {
    match args.get(key) {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("{key}: {e}")),
        None => Err(format!("missing field: {key}")),
    }
}

fn case_or_default(args: &Value, default: &str) -> String {
    args.get("case")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| default.to_string())
}

/// The tool catalogue, parsed once. Named exactly; the schemas state the typed shapes
/// the handlers deserialize into.
fn tools_catalog() -> &'static Value {
    static TOOLS: OnceLock<Value> = OnceLock::new();
    TOOLS.get_or_init(|| {
        serde_json::from_str(TOOLS_JSON)
            .expect("the tool catalogue is a constant with known-good JSON")
    })
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

const TOOLS_JSON: &str = r#"[
 {"name":"open","description":"Open the session's document (.fsm text or base64 .fsmz). Exactly one of doc/docz.","inputSchema":{"type":"object","properties":{"doc":{"type":"string"},"docz":{"type":"string"}}}},
 {"name":"inspect","description":"The open document's analysis setup: regions, supports, loads, temperatures, material, fill, solve settings, validate problems, missing mesh grids.","inputSchema":{"type":"object","additionalProperties":false}},
 {"name":"add_region","description":"Add a named region (union of shape-face selectors).","inputSchema":{"type":"object","required":["selectors"],"properties":{"id":{"type":"string"},"selectors":{"type":"array","items":{"type":"object","required":["shape","face"],"properties":{"shape":{"type":"string"},"face":{"type":"string"},"patch":{"type":"integer"}}}}}}},
 {"name":"add_support","description":"Fix displacement components on a region.","inputSchema":{"type":"object","required":["region","fixed"],"properties":{"id":{"type":"string"},"region":{"type":"string"},"fixed":{"type":"array","items":{"type":"boolean"},"minItems":3,"maxItems":3}}}},
 {"name":"add_load","description":"Apply a load (force/pressure/distributed) on a region for a load case.","inputSchema":{"type":"object","required":["region","kind"],"properties":{"id":{"type":"string"},"region":{"type":"string"},"case":{"type":"string"},"kind":{"type":"object"}}}},
 {"name":"add_temperature","description":"Apply a fixed boundary temperature on a region for a thermal case.","inputSchema":{"type":"object","required":["region","celsius"],"properties":{"id":{"type":"string"},"region":{"type":"string"},"celsius":{"type":"number"},"case":{"type":"string"}}}},
 {"name":"set_material","description":"Assign a material label to a node.","inputSchema":{"type":"object","required":["node","material"],"properties":{"node":{"type":"string"},"material":{"type":"string"}}}},
 {"name":"set_let","description":"Set a named `let` variable (or dotted parameter address) and re-evaluate dependent expressions.","inputSchema":{"type":"object","required":["name","value"],"properties":{"name":{"type":"string"},"value":{"type":"number"}}}},
 {"name":"set_fill","description":"Configure or update the document's TPMS lattice fill (including `skin_max` and `rotate`), or clear it (`clear: true`).","inputSchema":{"type":"object","properties":{"clear":{"type":"boolean"},"family":{"type":"string","enum":["gyroid","primitive","diamond"]},"mode":{"type":"string","enum":["sheet","network"]},"cell":{"type":"number"},"skin":{"type":"number"},"skin_max":{"type":["number","null"]},"rho_min":{"type":"number"},"rho_max":{"type":"number"},"curve":{"type":"number"},"min_wall":{"type":"number"},"protect":{"type":"array","items":{"type":"string"}},"margin":{"type":"number"},"case":{"type":["string","null"]},"rotate":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3}}}},
 {"name":"set_build","description":"Set or clear the additive build orientation `solve build=(theta_deg, phi_deg)` and optional target element `size`.","inputSchema":{"type":"object","properties":{"clear":{"type":"boolean"},"angles":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},"theta":{"type":"number"},"phi":{"type":"number"},"size":{"type":["number","null"]}}}},
 {"name":"doc_text","description":"The open document as .fsm text (a print of the current model, edits included).","inputSchema":{"type":"object","additionalProperties":false}},
 {"name":"solve","description":"Run the API question (solve/ladder/modes/sweep/overhang) on the session's document; returns the same JSON envelope the `fieldsmith api` verb writes.","inputSchema":{"type":"object","required":["kind"],"properties":{"kind":{"type":"string","enum":["solve","ladder","modes","sweep","overhang"]},"dofs":{"type":"integer"},"h":{"type":"number"},"n":{"type":"integer"},"rungs":{"type":"array","items":{"type":"integer"}},"tolerance":{"type":"string"},"budget":{"type":"string"},"param":{"type":"string"},"values":{"type":"array","items":{"type":"number"}},"build":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"threshold_deg":{"type":"number"},"equilibrate":{"type":"boolean"}}}},
 {"name":"export","description":"Mesh or slice-stack the session document (facts, and artifact bytes base64 when stl/mf are requested; pass `slices: true` for 2D `<s:slicestack>` export).","inputSchema":{"type":"object","properties":{"h":{"type":"number"},"layer_h":{"type":"number"},"slices":{"type":"boolean"},"adaptive":{"type":"boolean"},"solid_only":{"type":"boolean"},"stl":{"type":"boolean"},"mf":{"type":"boolean"}}}}
]"#;
