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

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use fieldsmith_api::schema::{QuestionSpec, APIV};
use fieldsmith_api::{b64, export, load_doc, missing_grids, question, respond};
use fieldsmith_core::fsmz;
use fieldsmith_doc::{
    canonical_color, fsm, param, Convection, Document, Family, FillMode, HeatFlux, Let, Load,
    LoadKind, Node, NodeKind, Op, Region, Selector, Support, Temperature, Transform,
};
use fieldsmith_solve::coupled_system::{
    CoupledEngineeringSystem, DesignParameter, EngineConfig, MonolithicSystemEngine, NamedScenario,
    ObjectiveDescriptor, ObjectiveKind, RegenerativeCooledNozzleSystem, SystemAnalysisResult,
};
use fieldsmith_solve::float_bath::{CffCase, FloatBathCoaterDesignSystem};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "fieldsmith-mcp";

/// Shared workspace state persisted to `FIELDSMITH_WORKSPACE` (`.fieldsmith-workspace.json`)
/// when configured, enabling real-time synchronisation between MCP agents, REST API calls,
/// and the interactive Web 3D CAD + Coupled Multi-Physics Workbench.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SharedWorkspaceState {
    pub revision: u64,
    pub updated_at_ms: u64,
    pub actor: String,
    pub last_tool: String,
    pub doc_fsm: Option<String>,
    pub coupled_system_id: Option<String>,
    pub design_p: Option<Vec<f64>>,
    pub epistemic_theta: Option<Vec<f64>>,
    #[serde(default)]
    pub activity_log: Vec<Value>,
}

pub struct Server {
    doc: Option<Document>,
    workspace_path: Option<PathBuf>,
    coupled_system_id: Option<String>,
    design_p: Option<Vec<f64>>,
    epistemic_theta: Option<Vec<f64>>,
}

impl Server {
    pub fn new() -> Self {
        let workspace_path = std::env::var("FIELDSMITH_WORKSPACE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from);
        let mut srv = Self {
            doc: None,
            workspace_path,
            coupled_system_id: None,
            design_p: None,
            epistemic_theta: None,
        };
        srv.load_workspace_if_present();
        srv
    }

    pub fn with_workspace(path: impl Into<PathBuf>) -> Self {
        let mut srv = Self {
            doc: None,
            workspace_path: Some(path.into()),
            coupled_system_id: None,
            design_p: None,
            epistemic_theta: None,
        };
        srv.load_workspace_if_present();
        srv
    }

    fn load_workspace_if_present(&mut self) {
        let Some(path) = &self.workspace_path else {
            return;
        };
        let Ok(raw) = std::fs::read_to_string(path) else {
            return;
        };
        let Ok(ws) = serde_json::from_str::<SharedWorkspaceState>(&raw) else {
            return;
        };
        if let Some(ref text) = ws.doc_fsm {
            if let Ok(d) = fsm::parse(text) {
                self.doc = Some(d);
            }
        }
        self.coupled_system_id = ws.coupled_system_id;
        self.design_p = ws.design_p;
        self.epistemic_theta = ws.epistemic_theta;
    }

    fn save_workspace_after_tool(&mut self, tool_name: &str, summary: &str) {
        let Some(path) = &self.workspace_path else {
            return;
        };
        let mut ws = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<SharedWorkspaceState>(&s).ok())
            .unwrap_or_default();
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        ws.revision += 1;
        ws.updated_at_ms = now_ms;
        ws.actor = "mcp-agent".to_string();
        ws.last_tool = tool_name.to_string();
        if let Some(ref d) = self.doc {
            ws.doc_fsm = Some(fsm::print(d));
        }
        if self.coupled_system_id.is_some() {
            ws.coupled_system_id = self.coupled_system_id.clone();
        }
        if self.design_p.is_some() {
            ws.design_p = self.design_p.clone();
        }
        if self.epistemic_theta.is_some() {
            ws.epistemic_theta = self.epistemic_theta.clone();
        }
        ws.activity_log.push(json!({
            "revision": ws.revision,
            "timestamp_ms": now_ms,
            "actor": "mcp-agent",
            "tool": tool_name,
            "summary": summary,
        }));
        if ws.activity_log.len() > 25 {
            let drain = ws.activity_log.len() - 25;
            ws.activity_log.drain(0..drain);
        }
        if let Ok(serialized) = serde_json::to_string_pretty(&ws) {
            let _ = std::fs::write(path, serialized);
        }
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
        self.load_workspace_if_present();
        let res = match name {
            "open" | "load_fsm" => self.open(args),
            "new_document" => self.new_document(args),
            "inspect" => self.inspect(args),
            "list_shapes" => self.list_shapes(args),
            "upsert_shape" | "add_primitive" => self.upsert_shape(args),
            "delete_entity" => self.delete_entity(args),
            "add_region" => self.add_region(args),
            "add_support" => self.add_support(args),
            "add_load" => self.add_load(args),
            "add_temperature" => self.add_temperature(args),
            "add_convection" => self.add_convection(args),
            "add_heatflux" => self.add_heatflux(args),
            "set_material" => self.set_material(args),
            "set_let" | "set_param" => self.set_let(args),
            "set_fill" => self.set_fill(args),
            "set_build" => self.set_build(args),
            "doc_text" | "export_fsm" => self.doc_text(args),
            "solve" | "solve_study" => self.solve(args),
            "export" => self.export(args),
            "list_coupled_systems" => self.list_coupled_systems(args),
            "solve_coupled_system" => self.solve_coupled_system(args),
            "optimise_coupled_system" => self.optimise_coupled_system(args),
            _ => Err(format!("unknown tool: {name}")),
        };
        if res.is_ok() {
            let is_mutating = matches!(
                name,
                "open"
                    | "load_fsm"
                    | "new_document"
                    | "upsert_shape"
                    | "add_primitive"
                    | "delete_entity"
                    | "add_region"
                    | "add_support"
                    | "add_load"
                    | "add_temperature"
                    | "add_convection"
                    | "add_heatflux"
                    | "set_material"
                    | "set_let"
                    | "set_param"
                    | "set_fill"
                    | "set_build"
                    | "solve_coupled_system"
                    | "optimise_coupled_system"
            );
            if is_mutating {
                self.save_workspace_after_tool(name, &format!("Executed MCP tool `{name}`"));
            }
        }
        res
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

    /// Exactly one of doc/docz (or fsm as alias for doc), through the shared loader;
    /// the open is the session's state. A `doc` text import carries no grids, so
    /// `missing_grids` names any mesh the session cannot yet evaluate.
    fn open(&mut self, args: Value) -> Result<Value, String> {
        let doc = args
            .get("doc")
            .or_else(|| args.get("fsm"))
            .and_then(Value::as_str);
        let docz = args.get("docz").and_then(Value::as_str);
        if doc.is_none() && docz.is_none() {
            return Err("open: pass exactly one of doc/fsm (.fsm text) or docz (base64 .fsmz)".into());
        }
        let d = load_doc(doc, docz)?;
        let summary = doc_summary(&d);
        self.doc = Some(d);
        Ok(summary)
    }

    /// Reset workspace to a clean .fsm document with optional name and material.
    fn new_document(&mut self, args: Value) -> Result<Value, String> {
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Untitled")
            .to_string();
        let material = args
            .get("material")
            .and_then(Value::as_str)
            .unwrap_or("petg")
            .to_string();
        let mut d = Document::default();
        d.name = name;
        d.material = material;
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

    fn add_convection(&mut self, args: Value) -> Result<Value, String> {
        let region = text_field("region", &args)?;
        let h = number_field("h", &args)?;
        let t_inf = args
            .get("t_inf")
            .or_else(|| args.get("tinf"))
            .and_then(Value::as_f64)
            .ok_or_else(|| "add_convection: missing t_inf/tinf".to_string())?;
        let case = case_or_default(&args, "Thermal 1");
        self.add_entity("conv", args, |d, id| {
            d.convections.push(Convection {
                id,
                region,
                h,
                t_inf,
                case,
            });
            Ok(())
        })
    }

    fn add_heatflux(&mut self, args: Value) -> Result<Value, String> {
        let region = text_field("region", &args)?;
        let q = number_field("q", &args)?;
        let case = case_or_default(&args, "Thermal 1");
        self.add_entity("hf", args, |d, id| {
            d.heat_fluxes.push(HeatFlux {
                id,
                region,
                q,
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
        let name = text_field("name", &args)
            .or_else(|_| text_field("addr", &args))?;
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
        let mut spec_val = args;
        if spec_val.get("kind").is_none() {
            spec_val["kind"] = json!("solve");
        }
        let spec: QuestionSpec =
            serde_json::from_value(spec_val).map_err(|e| format!("solve: {e}"))?;
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

    /// Inspect the open document's full 3D CSG/SDF shape tree (`root`), `lets`, and `consumed` expressions.
    fn list_shapes(&mut self, _args: Value) -> Result<Value, String> {
        let d = self.doc()?;
        Ok(json!({
            "name": d.name,
            "material": d.material,
            "lets": d.lets,
            "consumed": d.consumed,
            "shapes": d.root,
        }))
    }

    /// Create or update a 3D CAD primitive shape (`box`, `cylinder`, `sphere`, `capsule`, `torus`)
    /// in the open document's root CSG/SDF tree.
    fn upsert_shape(&mut self, args: Value) -> Result<Value, String> {
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let kind_str = text_field("kind", &args)?;
        let id = match args.get("id").and_then(Value::as_str) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => d.fresh_id(&kind_str),
        };
        if !fsm::is_ident(&id) {
            return Err(format!("upsert_shape: {id:?} is not a valid shape identifier"));
        }

        let op = match args.get("op").and_then(Value::as_str).unwrap_or("add") {
            "add" => Op::Add,
            "subtract" => Op::Subtract,
            "intersect" => Op::Intersect,
            other => return Err(format!("upsert_shape: unknown op {other:?} (expected add|subtract|intersect)")),
        };
        let blend = args.get("blend").and_then(Value::as_f64).unwrap_or(0.0);
        let suppressed = flag(&args, "suppressed");
        let translate: [f64; 3] = match args.get("at") {
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("upsert_shape: at: {e}"))?,
            None => [0.0, 0.0, 0.0],
        };
        let rotate: [f64; 3] = match args.get("rotate") {
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("upsert_shape: rotate: {e}"))?,
            None => [0.0, 0.0, 0.0],
        };
        let material = match args.get("material") {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        };
        let color = match args.get("color") {
            Some(Value::String(s)) if !s.is_empty() => Some(
                canonical_color(s)
                    .ok_or_else(|| format!("upsert_shape: invalid hex color {s:?}"))?,
            ),
            _ => None,
        };
        let round = args.get("round").and_then(Value::as_f64).unwrap_or(0.0);

        let kind = match kind_str.as_str() {
            "box" => {
                let size: [f64; 3] = field("size", &args)?;
                NodeKind::Box { size, round }
            }
            "cylinder" => {
                let radius = number_field("radius", &args)?;
                let height = number_field("height", &args)?;
                NodeKind::Cylinder {
                    radius,
                    height,
                    round,
                }
            }
            "sphere" => {
                let radius = number_field("radius", &args)?;
                NodeKind::Sphere { radius }
            }
            "capsule" => {
                let radius = number_field("radius", &args)?;
                let length = number_field("length", &args)?;
                NodeKind::Capsule { radius, length }
            }
            "torus" => {
                let major = number_field("major", &args)?;
                let minor = number_field("minor", &args)?;
                NodeKind::Torus { major, minor }
            }
            other => {
                return Err(format!(
                    "upsert_shape: unknown shape kind {other:?} (expected box|cylinder|sphere|capsule|torus)"
                ))
            }
        };

        let node = Node {
            id: id.clone(),
            op,
            blend,
            suppressed,
            transform: Transform { translate, rotate },
            material,
            color,
            kind,
        };

        let action = if let Some(slot) = d.root.iter().position(|n| n.id == id) {
            d.root[slot] = node;
            "updated"
        } else {
            d.root.push(node);
            "created"
        };
        let problems = d.validate();
        Ok(json!({
            "id": id,
            "action": action,
            "shapes": d.root.len(),
            "problems": problems,
        }))
    }

    /// Delete a shape, region, support, load, or temperature by `id`.
    fn delete_entity(&mut self, args: Value) -> Result<Value, String> {
        let id = text_field("id", &args)?;
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let n_shapes = d.root.len();
        d.root.retain(|n| n.id != id);
        if d.root.len() < n_shapes {
            let problems = d.validate();
            return Ok(json!({"deleted": id, "kind": "shape", "problems": problems}));
        }
        let n_regions = d.regions.len();
        d.regions.retain(|r| r.id != id);
        if d.regions.len() < n_regions {
            let problems = d.validate();
            return Ok(json!({"deleted": id, "kind": "region", "problems": problems}));
        }
        let n_supports = d.supports.len();
        d.supports.retain(|s| s.id != id);
        if d.supports.len() < n_supports {
            let problems = d.validate();
            return Ok(json!({"deleted": id, "kind": "support", "problems": problems}));
        }
        let n_loads = d.loads.len();
        d.loads.retain(|l| l.id != id);
        if d.loads.len() < n_loads {
            let problems = d.validate();
            return Ok(json!({"deleted": id, "kind": "load", "problems": problems}));
        }
        let n_temps = d.temperatures.len();
        d.temperatures.retain(|t| t.id != id);
        if d.temperatures.len() < n_temps {
            let problems = d.validate();
            return Ok(json!({"deleted": id, "kind": "temperature", "problems": problems}));
        }
        Err(format!("delete_entity: no entity named {id:?}"))
    }

    /// List all registered pluggable [`CoupledEngineeringSystem`] domain packs, their design levers,
    /// epistemic uncertainties, engineering objectives, and reference scenarios.
    fn list_coupled_systems(&mut self, _args: Value) -> Result<Value, String> {
        let case = CffCase::uk5_reference()?;
        let bath_sys = FloatBathCoaterDesignSystem::new(case, 10);
        let nozzle_sys = RegenerativeCooledNozzleSystem::default();

        let bath_scenarios = vec![
            NamedScenario {
                name: "A. Baseline High-H₂ (115 mm sill, 10% H₂)".to_string(),
                description: "Conventional open front sill with high 10% H₂ atmosphere to suppress SnOₓ".to_string(),
                design_p: vec![115.0, 24.0, 48.0, 10.0, 210.0, -4.0],
            },
            NamedScenario {
                name: "B. Naive Low-H₂ (115 mm sill, 3.8% H₂)".to_string(),
                description: "Cuts H₂ to 3.8% without hydrodynamic sill barrier — triggers SnO₂ bloom & optical waves".to_string(),
                design_p: vec![115.0, 24.0, 48.0, 3.8, 210.0, -4.0],
            },
            NamedScenario {
                name: "C. Adjoint-Synthesised Sill + Coater (65 mm sill, 4.2% H₂)".to_string(),
                description: "65 mm sill + 28.5 m coater position + 260 A Lorentz damping + +1.5° cant satisfies all p90 gates at 58% lower H₂".to_string(),
                design_p: vec![65.0, 28.5, 34.0, 4.2, 260.0, 1.5],
            },
        ];

        let nozzle_scenarios = vec![
            NamedScenario {
                name: "A. Baseline Uncoated Liner (30 μm TBC, 0.42 kg/s H₂)".to_string(),
                description: "Thin 30 μm TBC with 2.4 mm channels — violates copper liner T_wall and compressive yield limits".to_string(),
                design_p: vec![2.4, 1.2, 30.0, 0.42],
            },
            NamedScenario {
                name: "B. Brute-Force High Coolant Flow (30 μm TBC, 0.72 kg/s H₂)".to_string(),
                description: "Pumps 0.72 kg/s H₂ through narrow channels — cools wall but wastes H₂ and spikes Δp".to_string(),
                design_p: vec![1.6, 1.2, 30.0, 0.72],
            },
            NamedScenario {
                name: "C. Graded TBC + High-Velocity Throat (145 μm TBC, 0.38 kg/s H₂)".to_string(),
                description: "Tailored 1.55 mm throat channel + 145 μm TBC reduces wall temperature and H₂ mass flow simultaneously".to_string(),
                design_p: vec![1.55, 0.95, 145.0, 0.38],
            },
        ];

        Ok(json!({
            "systems": [
                {
                    "system_id": bath_sys.system_id(),
                    "system_title": bath_sys.system_title(),
                    "industry_sector": bath_sys.industry_sector(),
                    "num_dofs": bath_sys.num_dofs(),
                    "fields": bath_sys.field_descriptors(),
                    "design_parameters": bath_sys.design_parameters(),
                    "epistemic_parameters": bath_sys.epistemic_parameters(),
                    "objectives": bath_sys.objective_descriptors(),
                    "scenarios": bath_scenarios,
                },
                {
                    "system_id": nozzle_sys.system_id(),
                    "system_title": nozzle_sys.system_title(),
                    "industry_sector": nozzle_sys.industry_sector(),
                    "num_dofs": nozzle_sys.num_dofs(),
                    "fields": nozzle_sys.field_descriptors(),
                    "design_parameters": nozzle_sys.design_parameters(),
                    "epistemic_parameters": nozzle_sys.epistemic_parameters(),
                    "objectives": nozzle_sys.objective_descriptors(),
                    "scenarios": nozzle_scenarios,
                }
            ]
        }))
    }

    /// Solve a registered [`CoupledEngineeringSystem`] at `design_p` and `epistemic_theta`,
    /// returning the converged monolithic state, Sparse Adjoint sensitivities `dΦ_m/dp_k`,
    /// `p10/p50/p90` epistemic intervals, and 3D `.fsm` CAD twin (automatically loaded into
    /// the open session document unless `sync_cad: false` is passed).
    fn solve_coupled_system(&mut self, args: Value) -> Result<Value, String> {
        let system_id = args
            .get("system_id")
            .and_then(Value::as_str)
            .or(self.coupled_system_id.as_deref())
            .unwrap_or("pilkington_float_bath_cvd_coater_v1")
            .to_string();
        let sync_cad = args.get("sync_cad").and_then(Value::as_bool).unwrap_or(true);
        let req_p: Option<Vec<f64>> = args
            .get("design_p")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .or_else(|| self.design_p.clone());
        let req_theta: Option<Vec<f64>> = args
            .get("epistemic_theta")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .or_else(|| self.epistemic_theta.clone());

        if system_id == "pilkington_float_bath_cvd_coater_v1" {
            let case = CffCase::uk5_reference()?;
            let sys = FloatBathCoaterDesignSystem::new(case, 10);
            let p = req_p
                .filter(|v| v.len() == sys.design_parameters().len())
                .unwrap_or_else(|| sys.nominal_design_vector());
            let theta = req_theta
                .filter(|v| v.len() == sys.epistemic_parameters().len())
                .unwrap_or_else(|| sys.nominal_epistemic_vector());
            let cfg = EngineConfig {
                dt_s: 3.0,
                num_bdf_steps: 1,
                max_newton_iters: 5,
                newton_tol: 1e-8,
                state_fd_eps: 1e-6,
            };
            let analysis = MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p, &theta)?;
            let obj_descs = sys.objective_descriptors();
            let pass_p50 = analysis.all_constraints_satisfied_p50(&obj_descs);
            let pass_p90 = analysis.all_constraints_satisfied_robust_p90(&obj_descs);
            let fsm_3d = sys.build_3d_fsm(&p);
            if sync_cad {
                if let Some(ref text) = fsm_3d {
                    let d = fsm::parse(text).map_err(|e| e.to_string())?;
                    self.doc = Some(d);
                }
            }
            self.coupled_system_id = Some(system_id);
            self.design_p = Some(p);
            self.epistemic_theta = Some(theta);
            Ok(json!({
                "analysis": analysis,
                "constraints_pass_p50": pass_p50,
                "constraints_pass_robust_p90": pass_p90,
                "synced_to_cad_doc": sync_cad && fsm_3d.is_some(),
                "fsm_3d": fsm_3d,
            }))
        } else if system_id == "regen_nozzle_h2_v1" {
            let sys = RegenerativeCooledNozzleSystem::default();
            let p = req_p
                .filter(|v| v.len() == sys.design_parameters().len())
                .unwrap_or_else(|| sys.nominal_design_vector());
            let theta = req_theta
                .filter(|v| v.len() == sys.epistemic_parameters().len())
                .unwrap_or_else(|| sys.nominal_epistemic_vector());
            let cfg = EngineConfig::default();
            let analysis = MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p, &theta)?;
            let obj_descs = sys.objective_descriptors();
            let pass_p50 = analysis.all_constraints_satisfied_p50(&obj_descs);
            let pass_p90 = analysis.all_constraints_satisfied_robust_p90(&obj_descs);
            let fsm_3d = sys.build_3d_fsm(&p);
            if sync_cad {
                if let Some(ref text) = fsm_3d {
                    let d = fsm::parse(text).map_err(|e| e.to_string())?;
                    self.doc = Some(d);
                }
            }
            self.coupled_system_id = Some(system_id);
            self.design_p = Some(p);
            self.epistemic_theta = Some(theta);
            Ok(json!({
                "analysis": analysis,
                "constraints_pass_p50": pass_p50,
                "constraints_pass_robust_p90": pass_p90,
                "synced_to_cad_doc": sync_cad && fsm_3d.is_some(),
                "fsm_3d": fsm_3d,
            }))
        } else {
            Err(format!("solve_coupled_system: unknown system_id {system_id:?}"))
        }
    }

    /// Take `steps` (default `1`) automatic Sparse Adjoint `-dΦ/dp` design optimisation steps
    /// on a [`CoupledEngineeringSystem`], updating `design_p` and synthesising the updated
    /// 3D `.fsm` CAD twin into the session document.
    fn optimise_coupled_system(&mut self, args: Value) -> Result<Value, String> {
        let system_id = args
            .get("system_id")
            .and_then(Value::as_str)
            .or(self.coupled_system_id.as_deref())
            .unwrap_or("pilkington_float_bath_cvd_coater_v1")
            .to_string();
        let steps = args
            .get("steps")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .clamp(1, 20) as usize;
        let sync_cad = args.get("sync_cad").and_then(Value::as_bool).unwrap_or(true);
        let req_p: Option<Vec<f64>> = args
            .get("design_p")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .or_else(|| self.design_p.clone());
        let req_theta: Option<Vec<f64>> = args
            .get("epistemic_theta")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .or_else(|| self.epistemic_theta.clone());

        if system_id == "pilkington_float_bath_cvd_coater_v1" {
            let case = CffCase::uk5_reference()?;
            let sys = FloatBathCoaterDesignSystem::new(case, 10);
            let cfg = EngineConfig {
                dt_s: 3.0,
                num_bdf_steps: 1,
                max_newton_iters: 5,
                newton_tol: 1e-8,
                state_fd_eps: 1e-6,
            };
            self.run_adjoint_optimisation_loop(&sys, &cfg, req_p, req_theta, steps, sync_cad)
        } else if system_id == "regen_nozzle_h2_v1" {
            let sys = RegenerativeCooledNozzleSystem::default();
            let cfg = EngineConfig::default();
            self.run_adjoint_optimisation_loop(&sys, &cfg, req_p, req_theta, steps, sync_cad)
        } else {
            Err(format!("optimise_coupled_system: unknown system_id {system_id:?}"))
        }
    }

    fn run_adjoint_optimisation_loop<S: CoupledEngineeringSystem>(
        &mut self,
        sys: &S,
        cfg: &EngineConfig,
        req_p: Option<Vec<f64>>,
        req_theta: Option<Vec<f64>>,
        steps: usize,
        sync_cad: bool,
    ) -> Result<Value, String> {
        let dps = sys.design_parameters();
        let obj_descs = sys.objective_descriptors();
        let mut p = req_p
            .filter(|v| v.len() == dps.len())
            .unwrap_or_else(|| sys.nominal_design_vector());
        let theta = req_theta
            .filter(|v| v.len() == sys.epistemic_parameters().len())
            .unwrap_or_else(|| sys.nominal_epistemic_vector());

        let initial_p = p.clone();
        let mut initial_objectives = Vec::new();
        let mut last_analysis: Option<SystemAnalysisResult> = None;

        for step_idx in 0..steps {
            let analysis = MonolithicSystemEngine::solve_and_analyze(sys, cfg, &p, &theta)?;
            if step_idx == 0 {
                initial_objectives = analysis.objectives.clone();
            }
            p = compute_adjoint_step(&dps, &obj_descs, &p, &analysis);
            last_analysis = Some(analysis);
        }

        let final_analysis = MonolithicSystemEngine::solve_and_analyze(sys, cfg, &p, &theta)?;
        let pass_p50 = final_analysis.all_constraints_satisfied_p50(&obj_descs);
        let pass_p90 = final_analysis.all_constraints_satisfied_robust_p90(&obj_descs);
        let fsm_3d = sys.build_3d_fsm(&p);
        if sync_cad {
            if let Some(ref text) = fsm_3d {
                let d = fsm::parse(text).map_err(|e| e.to_string())?;
                self.doc = Some(d);
            }
        }
        self.coupled_system_id = Some(sys.system_id().to_string());
        self.design_p = Some(p.clone());
        self.epistemic_theta = Some(theta);

        let _ = last_analysis;
        Ok(json!({
            "system_id": sys.system_id(),
            "steps": steps,
            "initial_design_p": initial_p,
            "final_design_p": p,
            "initial_objectives": initial_objectives,
            "final_objectives": final_analysis.objectives,
            "constraints_pass_p50": pass_p50,
            "constraints_pass_robust_p90": pass_p90,
            "synced_to_cad_doc": sync_cad && fsm_3d.is_some(),
            "fsm_3d": fsm_3d,
            "analysis": final_analysis,
        }))
    }
}

fn compute_adjoint_step(
    design_parameters: &[DesignParameter],
    objectives: &[ObjectiveDescriptor],
    design_p: &[f64],
    analysis: &SystemAnalysisResult,
) -> Vec<f64> {
    let mut next_p = design_p.to_vec();
    for (k, dp) in design_parameters.iter().enumerate() {
        let span = dp.max_bound - dp.min_bound;
        let mut grad_norm: f64 = 0.0;
        for (m, obj) in objectives.iter().enumerate() {
            let val = analysis.objectives[m];
            let scale = val.abs().max(1.0);
            let g = analysis
                .design_sensitivities
                .get(m)
                .and_then(|row| row.get(k))
                .copied()
                .unwrap_or(0.0)
                * (span / scale);
            match obj.kind {
                ObjectiveKind::Minimize => {
                    grad_norm += g;
                }
                ObjectiveKind::Maximize => {
                    grad_norm -= g;
                }
                ObjectiveKind::UpperLimit(lim) => {
                    if val > lim {
                        grad_norm += 2.5 * g;
                    } else {
                        grad_norm += 0.35 * g;
                    }
                }
                ObjectiveKind::LowerLimit(lim) => {
                    if val < lim {
                        grad_norm -= 2.5 * g;
                    }
                }
                ObjectiveKind::TargetBand { min, max } => {
                    let mid = 0.5 * (min + max);
                    if val < min {
                        grad_norm -= 2.5 * g;
                    } else if val > max {
                        grad_norm += 2.5 * g;
                    } else {
                        grad_norm += 0.25 * (val - mid).signum() * g;
                    }
                }
            }
        }
        if grad_norm.abs() > 1e-9 {
            let step = -0.08 * span * grad_norm.clamp(-1.0, 1.0);
            next_p[k] = (next_p[k] + step).clamp(dp.min_bound, dp.max_bound);
        }
    }
    next_p
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
 {"name":"list_shapes","description":"Inspect the open document's 3D CSG/SDF shape tree (`shapes`), `lets` variables, and `consumed` expressions.","inputSchema":{"type":"object","additionalProperties":false}},
 {"name":"upsert_shape","description":"Create or update a 3D CAD primitive shape (`box`, `cylinder`, `sphere`, `capsule`, `torus`) in the open document's root CSG/SDF tree.","inputSchema":{"type":"object","required":["kind"],"properties":{"id":{"type":"string"},"kind":{"type":"string","enum":["box","cylinder","sphere","capsule","torus"]},"op":{"type":"string","enum":["add","subtract","intersect"]},"blend":{"type":"number"},"at":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"rotate":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"size":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"radius":{"type":"number"},"height":{"type":"number"},"length":{"type":"number"},"major":{"type":"number"},"minor":{"type":"number"},"round":{"type":"number"},"color":{"type":"string"},"material":{"type":"string"},"suppressed":{"type":"boolean"}}}},
 {"name":"delete_entity","description":"Delete a shape, region, support, load, or temperature by `id`.","inputSchema":{"type":"object","required":["id"],"properties":{"id":{"type":"string"}}}},
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
 {"name":"export","description":"Mesh or slice-stack the session document (facts, and artifact bytes base64 when stl/mf are requested; pass `slices: true` for 2D `<s:slicestack>` export).","inputSchema":{"type":"object","properties":{"h":{"type":"number"},"layer_h":{"type":"number"},"slices":{"type":"boolean"},"adaptive":{"type":"boolean"},"solid_only":{"type":"boolean"},"stl":{"type":"boolean"},"mf":{"type":"boolean"}}}},
 {"name":"list_coupled_systems","description":"List all registered pluggable CoupledEngineeringSystem multi-physics packs, design parameters, epistemic uncertainties, objectives, and reference scenarios.","inputSchema":{"type":"object","additionalProperties":false}},
 {"name":"solve_coupled_system","description":"Solve a CoupledEngineeringSystem at `design_p` and `epistemic_theta` (monolithic Newton + Sparse Adjoint `dΦ/dp` + FOSM `p10/p50/p90` UQ) and optionally sync the 3D `.fsm` CAD twin into the open session document (`sync_cad: true`).","inputSchema":{"type":"object","properties":{"system_id":{"type":"string","enum":["pilkington_float_bath_cvd_coater_v1","regen_nozzle_h2_v1"]},"design_p":{"type":"array","items":{"type":"number"}},"epistemic_theta":{"type":"array","items":{"type":"number"}},"sync_cad":{"type":"boolean"}}}},
 {"name":"optimise_coupled_system","description":"Take `steps` (default 1) Sparse Adjoint `-dΦ/dp` design optimisation steps on a CoupledEngineeringSystem and sync the synthesised 3D `.fsm` CAD twin into the session document.","inputSchema":{"type":"object","properties":{"system_id":{"type":"string","enum":["pilkington_float_bath_cvd_coater_v1","regen_nozzle_h2_v1"]},"design_p":{"type":"array","items":{"type":"number"}},"epistemic_theta":{"type":"array","items":{"type":"number"}},"steps":{"type":"integer"},"sync_cad":{"type":"boolean"}}}}
]"#;
