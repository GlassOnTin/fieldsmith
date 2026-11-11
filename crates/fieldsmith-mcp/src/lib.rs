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
use fieldsmith_core::channel::{
    analyze_sdf_channel as run_sdf_channel, bind_channel_bcs_to_region, ChannelAxis,
    ChannelFlowSpec, PassageMode,
};
use fieldsmith_core::declarative_system::DeclarativeCoupledSystem;
use fieldsmith_core::fsmz;
use fieldsmith_doc::command::{apply_doc_command, DocCommand};
use fieldsmith_doc::fixings::{insert_fixing_into_document, FixingInsertRequest, FixingKind};
use fieldsmith_doc::sketch::{
    preset_sketch_template, solve_sketch as solve_sketch_doc, ParametricSketch,
    SketchFeatureRequest,
};
use fieldsmith_doc::{
    canonical_color, fsm, materials, Document, Family, FillMode, LoadKind, MaterialRole, Node,
    NodeKind, Op, PlasticityLaw, ProfileVertex, Selector, Transform,
};
use fieldsmith_solve::antenna::{
    analyze_drone_antenna, build_deadcat_drone_fsm, optimise_drone_antenna,
    DeadCatGeminiAntennaSystem, DroneAntennaDesignSpec, DroneAntennaTopology,
};
use fieldsmith_solve::candle_flame::SolarCandleIgnitionSystem;
use fieldsmith_solve::coupled_system::{
    AnnularSwirlSprayCombustorSystem, CoupledEngineeringSystem, DesignParameter, EngineConfig,
    MonolithicSystemEngine, NamedScenario, ObjectiveDescriptor, ObjectiveKind,
    RegenerativeCooledNozzleSystem, SystemAnalysisResult,
};
use fieldsmith_solve::fixings::FastenerFixingSystem;
use fieldsmith_solve::float_bath::{CffCase, FloatBathCoaterDesignSystem};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub mod http;

pub const PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "fieldsmith-mcp";

/// Start the embedded HTTP compute + MCP server on `127.0.0.1:{port_hint}` (pass `0` for an
/// OS-assigned ephemeral port) and return the bound TCP port, or `-1` on error.
/// Used by the Android F-Droid wrapper (`sh.fieldsmith.app`) to launch the in-process native
/// `arm64-v8a` / `x86_64` solver and COOP/COEP asset host.
#[no_mangle]
pub extern "C" fn fieldsmith_start_local_server(port_hint: u16) -> i32 {
    let ws_path = std::env::var("FIELDSMITH_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("fieldsmith-workspace.json"));
    let web_dir = std::env::var("FIELDSMITH_WEB_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from);
    let cfg = http::ServeConfig {
        bind_addr: format!("127.0.0.1:{port_hint}"),
        workspace_path: ws_path,
        web_dir,
    };
    match http::spawn_http_server(cfg) {
        Ok(srv) => i32::from(srv.local_addr().port()),
        Err(_) => -1,
    }
}

/// Android JNI entry point for `sh.fieldsmith.app.FieldsmithNativeBridge.startLocalServer(portHint: Int): Int`.
/// Uses primitive `jint` (`i32`) parameter and return types so no external `jni` crate is needed.
#[no_mangle]
pub extern "system" fn Java_sh_fieldsmith_app_FieldsmithNativeBridge_startLocalServer(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    port_hint: i32,
) -> i32 {
    let p = u16::try_from(port_hint.max(0)).unwrap_or(0);
    fieldsmith_start_local_server(p)
}

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
    pub remote_compute_url: Option<String>,
    #[serde(default)]
    pub activity_log: Vec<Value>,
}

pub struct Server {
    doc: Option<Document>,
    workspace_path: Option<PathBuf>,
    coupled_system_id: Option<String>,
    design_p: Option<Vec<f64>>,
    epistemic_theta: Option<Vec<f64>>,
    remote_compute_url: Option<String>,
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
            remote_compute_url: None,
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
            remote_compute_url: None,
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
        self.remote_compute_url = ws.remote_compute_url;
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
        ws.remote_compute_url = self.remote_compute_url.clone();
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
            "insert_fixing" => self.insert_fixing(args),
            "solve_sketch" => self.solve_sketch(args),
            "upsert_sketch" | "save_sketch" => self.upsert_sketch(args),
            "apply_sketch_feature" => self.apply_sketch_feature(args),
            "apply_doc_command" => self.exec_doc_command(args),
            "delete_entity" => self.delete_entity(args),
            "add_region" => self.add_region(args),
            "add_support" => self.add_support(args),
            "add_load" => self.add_load(args),
            "add_temperature" => self.add_temperature(args),
            "add_convection" => self.add_convection(args),
            "add_heatflux" => self.add_heatflux(args),
            "set_material" => self.set_material(args),
            "upsert_custom_material" => self.upsert_custom_material(args),
            "set_let" | "set_param" => self.set_let(args),
            "set_fill" => self.set_fill(args),
            "set_build" => self.set_build(args),
            "upsert_assert" => self.upsert_assert(args),
            "delete_assert" => self.delete_assert(args),
            "doc_text" | "export_fsm" => self.doc_text(args),
            "solve" | "solve_study" => self.solve(args),
            "verify" => self.verify(args),
            "sensitivity" => self.sensitivity(args),
            "optimise" | "optimize" => self.optimise(args),
            "analyze_sdf_channel" => self.analyze_sdf_channel(args),
            "export" => self.export(args),
            "list_coupled_systems" => self.list_coupled_systems(args),
            "solve_coupled_system" => self.solve_coupled_system(args),
            "optimise_coupled_system" => self.optimise_coupled_system(args),
            "solve_drone_antenna" => self.solve_drone_antenna(args),
            "optimise_drone_antenna" => self.optimise_drone_antenna(args),
            "discover_compute_peers" => self.discover_compute_peers(args),
            "connect_compute_peer" => self.connect_compute_peer(args),
            "disconnect_compute_peer" => self.disconnect_compute_peer(args),
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
                    | "insert_fixing"
                    | "upsert_sketch"
                    | "save_sketch"
                    | "apply_sketch_feature"
                    | "apply_doc_command"
                    | "delete_entity"
                    | "add_region"
                    | "add_support"
                    | "add_load"
                    | "add_temperature"
                    | "add_convection"
                    | "add_heatflux"
                    | "set_material"
                    | "upsert_custom_material"
                    | "set_let"
                    | "set_param"
                    | "set_fill"
                    | "set_build"
                    | "upsert_assert"
                    | "delete_assert"
                    | "solve"
                    | "solve_study"
                    | "optimise"
                    | "optimize"
                    | "analyze_sdf_channel"
                    | "solve_coupled_system"
                    | "optimise_coupled_system"
                    | "solve_drone_antenna"
                    | "optimise_drone_antenna"
                    | "connect_compute_peer"
                    | "disconnect_compute_peer"
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
            return Err(
                "open: pass exactly one of doc/fsm (.fsm text) or docz (base64 .fsmz)".into(),
            );
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
        let d = Document {
            name,
            material,
            ..Default::default()
        };
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
            "coupled_system": d.coupled_system,
            "system_block": d.system_block,
            "asserts": d.asserts,
            "lets": d.lets,
            "sketches": d.sketches,
            "sketch_features": d.sketch_features,
            "custom_materials": d.custom_materials,
            "fill": fill,
            "solve": d.solve,
            "regions": d.regions,
            "supports": d.supports,
            "loads": d.loads,
            "temperatures": d.temperatures,
            "convections": d.convections,
            "heat_fluxes": d.heat_fluxes,
            "missing_grids": missing_grids(d),
            "problems": d.validate(),
        }))
    }

    fn add_region(&mut self, args: Value) -> Result<Value, String> {
        let selectors: Vec<Selector> = field("selectors", &args)?;
        let id = args.get("id").and_then(Value::as_str).map(str::to_string);
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let out = apply_doc_command(d, DocCommand::UpsertRegion { id, selectors })?;
        Ok(json!({"added": out.entity_id, "problems": out.problems}))
    }

    fn add_support(&mut self, args: Value) -> Result<Value, String> {
        let region = text_field("region", &args)?;
        let fixed: [bool; 3] = field("fixed", &args)?;
        let id = args.get("id").and_then(Value::as_str).map(str::to_string);
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let out = apply_doc_command(d, DocCommand::UpsertSupport { id, region, fixed })?;
        Ok(json!({"added": out.entity_id, "problems": out.problems}))
    }

    fn add_load(&mut self, args: Value) -> Result<Value, String> {
        let region = text_field("region", &args)?;
        let case = Some(case_or_default(&args, "Load 1"));
        let pressure_fn = args
            .get("pressure_fn")
            .or_else(|| args.get("expr"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let kind: LoadKind = if args.get("kind").is_some() {
            field("kind", &args)?
        } else if let Some(n) = args
            .get("preload")
            .or_else(|| args.get("newtons"))
            .and_then(Value::as_f64)
        {
            let axis: [f64; 3] = args
                .get("axis")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or([0.0; 3]);
            LoadKind::Preload { newtons: n, axis }
        } else if args.get("pressure").is_some()
            || args.get("mpa").is_some()
            || pressure_fn.is_some()
        {
            let p = args
                .get("pressure")
                .or_else(|| args.get("mpa"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            LoadKind::Pressure {
                mpa: p,
                expr: pressure_fn,
            }
        } else if let Some(v) = args
            .get("force")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            LoadKind::Force { vector: v }
        } else {
            return Err(
                "add_load: missing kind (or force / pressure / pressure_fn / preload)".into(),
            );
        };
        let id = args.get("id").and_then(Value::as_str).map(str::to_string);
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let out = apply_doc_command(
            d,
            DocCommand::UpsertLoad {
                id,
                region,
                case,
                kind,
            },
        )?;
        Ok(json!({"added": out.entity_id, "problems": out.problems}))
    }

    fn add_temperature(&mut self, args: Value) -> Result<Value, String> {
        let region = text_field("region", &args)?;
        let expr = args
            .get("temp_fn")
            .or_else(|| args.get("expr"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let celsius = match args
            .get("celsius")
            .or_else(|| args.get("temp"))
            .and_then(Value::as_f64)
        {
            Some(v) => v,
            None if expr.is_some() => 0.0,
            None => return Err("add_temperature: missing celsius (or temp_fn)".into()),
        };
        let case = Some(case_or_default(&args, "Thermal 1"));
        let id = args.get("id").and_then(Value::as_str).map(str::to_string);
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let out = apply_doc_command(
            d,
            DocCommand::UpsertTemperature {
                id,
                region,
                celsius,
                expr,
                case,
            },
        )?;
        Ok(json!({"added": out.entity_id, "problems": out.problems}))
    }

    fn add_convection(&mut self, args: Value) -> Result<Value, String> {
        let region = text_field("region", &args)?;
        let h_expr = args
            .get("h_fn")
            .or_else(|| args.get("h_expr"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let t_inf_expr = args
            .get("tinf_fn")
            .or_else(|| args.get("t_inf_fn"))
            .or_else(|| args.get("t_inf_expr"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let h = match args.get("h").and_then(Value::as_f64) {
            Some(v) => v,
            None if h_expr.is_some() => 0.0,
            None => return Err("add_convection: missing h (or h_fn)".into()),
        };
        let t_inf = match args
            .get("t_inf")
            .or_else(|| args.get("tinf"))
            .and_then(Value::as_f64)
        {
            Some(v) => v,
            None if t_inf_expr.is_some() => 0.0,
            None => return Err("add_convection: missing t_inf/tinf (or tinf_fn)".into()),
        };
        let case = Some(case_or_default(&args, "Thermal 1"));
        let id = args.get("id").and_then(Value::as_str).map(str::to_string);
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let out = apply_doc_command(
            d,
            DocCommand::UpsertConvection {
                id,
                region,
                h,
                t_inf,
                h_expr,
                t_inf_expr,
                case,
            },
        )?;
        Ok(json!({"added": out.entity_id, "problems": out.problems}))
    }

    fn add_heatflux(&mut self, args: Value) -> Result<Value, String> {
        let region = text_field("region", &args)?;
        let expr = args
            .get("q_fn")
            .or_else(|| args.get("expr"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let q = match args.get("q").and_then(Value::as_f64) {
            Some(v) => v,
            None if expr.is_some() => 0.0,
            None => return Err("add_heatflux: missing q (or q_fn)".into()),
        };
        let case = Some(case_or_default(&args, "Thermal 1"));
        let id = args.get("id").and_then(Value::as_str).map(str::to_string);
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let out = apply_doc_command(
            d,
            DocCommand::UpsertHeatFlux {
                id,
                region,
                q,
                expr,
                case,
            },
        )?;
        Ok(json!({"added": out.entity_id, "problems": out.problems}))
    }

    fn set_material(&mut self, args: Value) -> Result<Value, String> {
        let material = text_field("material", &args)?;
        let node = args
            .get("node")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let out = apply_doc_command(
            d,
            DocCommand::SetMaterial {
                material: material.clone(),
                node: node.clone(),
            },
        )?;
        if let Some(n) = node {
            Ok(json!({"node": n, "material": material, "problems": out.problems}))
        } else {
            Ok(json!({"material": material, "problems": out.problems}))
        }
    }

    fn upsert_custom_material(&mut self, args: Value) -> Result<Value, String> {
        let id = text_field("id", &args)?;
        let e = number_field("e", &args)?;
        let nu = number_field("nu", &args)?;
        let yield_strength = args
            .get("yield_strength")
            .or_else(|| args.get("yield"))
            .and_then(Value::as_f64)
            .ok_or_else(|| "upsert_custom_material: missing yield_strength/yield".to_string())?;
        let density = number_field("density", &args)?;
        let min_wall = args
            .get("min_wall")
            .or_else(|| args.get("wall"))
            .and_then(Value::as_f64)
            .unwrap_or(0.8);
        let role = match args
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("structural")
        {
            "structural" | "solid" | "" => MaterialRole::Structural,
            "fluid" => MaterialRole::Fluid,
            other => {
                return Err(format!(
                    "upsert_custom_material: invalid role {other:?} (expected structural|fluid)"
                ))
            }
        };
        let mat = materials::Material {
            id: id.clone(),
            name: args
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_string(),
            e,
            nu,
            yield_strength,
            density,
            min_wall,
            alpha: args
                .get("alpha")
                .or_else(|| args.get("a"))
                .and_then(Value::as_f64),
            k: args.get("k").and_then(Value::as_f64),
            yield_z: args.get("yield_z").and_then(Value::as_f64),
            shear_z: args.get("shear_z").and_then(Value::as_f64),
            hardening_n: args.get("hardening_n").and_then(Value::as_f64),
            hardening_k: args.get("hardening_k").and_then(Value::as_f64),
            weibull_m: args.get("weibull_m").and_then(Value::as_f64),
            role,
            source: args
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or("Custom user material")
                .to_string(),
        };
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let out = apply_doc_command(d, DocCommand::UpsertCustomMaterial { material: mat })?;
        Ok(json!({ "id": id, "action": out.action, "problems": out.problems }))
    }

    /// Update an existing `let` variable (or dotted parameter address `<id>.<attr>[.<n>]`),
    /// or define a new `let` variable, and re-evaluate all dependent consumer expressions
    /// and bound parametric sketch features.
    fn set_let(&mut self, args: Value) -> Result<Value, String> {
        let name = text_field("name", &args).or_else(|_| text_field("addr", &args))?;
        let value = number_field("value", &args)?;
        let expr = args
            .get("expr")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let is_dotted = name.contains('.');
        let out = apply_doc_command(
            d,
            DocCommand::SetLet {
                name: name.clone(),
                value,
                expr,
            },
        )?;
        if is_dotted {
            Ok(json!({"name": name, "value": value, "problems": out.problems}))
        } else {
            Ok(json!({"name": name, "value": value, "lets": d.lets, "problems": out.problems}))
        }
    }

    fn upsert_assert(&mut self, args: Value) -> Result<Value, String> {
        let index = args
            .get("index")
            .and_then(Value::as_u64)
            .map(|v| v as usize);
        let metric = text_field("metric", &args)?;
        let target = match args.get("target") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => return Err("upsert_assert: 'target' is required (string or number)".into()),
        };
        let label = args
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let tolerance = args
            .get("tolerance")
            .or_else(|| args.get("tol"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let unit = args
            .get("unit")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let out = apply_doc_command(
            d,
            DocCommand::UpsertAssert {
                index,
                assertion: fsm::LiterateAssert {
                    label,
                    metric,
                    target,
                    tolerance,
                    unit,
                },
            },
        )?;
        Ok(json!({
            "label": out.entity_id,
            "action": out.action,
            "asserts": d.asserts,
            "problems": out.problems,
        }))
    }

    fn delete_assert(&mut self, args: Value) -> Result<Value, String> {
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let idx = if let Some(i) = args.get("index").and_then(Value::as_u64) {
            i as usize
        } else if let Some(lbl) = args.get("label").and_then(Value::as_str) {
            d.asserts
                .iter()
                .position(|a| a.label == lbl)
                .ok_or_else(|| format!("delete_assert: no assert with label {lbl:?}"))?
        } else {
            return Err("delete_assert: provide 'index' or 'label'".into());
        };
        let out = apply_doc_command(d, DocCommand::DeleteAssert { index: idx })?;
        Ok(json!({
            "label": out.entity_id,
            "action": out.action,
            "asserts": d.asserts,
            "problems": out.problems,
        }))
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
        if let Some(fam) = args
            .get("family")
            .or_else(|| args.get("kind"))
            .and_then(Value::as_str)
        {
            f.family = match fam {
                "gyroid" => Family::Gyroid,
                "primitive" | "schwarz_p" => Family::Primitive,
                "diamond" => Family::Diamond,
                other => return Err(format!("set_fill: unknown lattice family {other:?}")),
            };
        }
        if let Some(m) = args.get("morph") {
            if m.is_null() {
                f.morph = None;
            } else if let Some(s) = m.as_str() {
                f.morph = match s {
                    "" | "none" => None,
                    "gyroid" => Some(Family::Gyroid),
                    "primitive" | "schwarz_p" => Some(Family::Primitive),
                    "diamond" => Some(Family::Diamond),
                    other => {
                        return Err(format!("set_fill: unknown morph lattice family {other:?}"))
                    }
                };
            } else {
                return Err("set_fill: morph must be a lattice family string or null".into());
            }
        }
        if let Some(mode) = args.get("mode").and_then(Value::as_str) {
            f.mode = match mode {
                "sheet" => FillMode::Sheet,
                "network" => FillMode::Network,
                other => return Err(format!("set_fill: unknown fill mode {other:?}")),
            };
        }
        if let Some(v) = args
            .get("cell")
            .or_else(|| args.get("cell_mm"))
            .and_then(Value::as_f64)
        {
            f.cell = v;
        }
        if let Some(cm) = args.get("cell_min") {
            if cm.is_null() {
                f.cell_min = None;
            } else if let Some(v) = cm.as_f64() {
                f.cell_min = if (v - f.cell).abs() <= 1e-9 {
                    None
                } else {
                    Some(v)
                };
            } else {
                return Err("set_fill: cell_min must be a number or null".into());
            }
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
        if let Some(er) = args.get("erode") {
            if er.is_null() {
                f.erode = None;
            } else if let Some(v) = er.as_f64() {
                if !v.is_finite() || v < 0.0 {
                    return Err(format!("set_fill: erode must be non-negative (got {v})"));
                }
                f.erode = if v <= 1e-9 { None } else { Some(v) };
            } else {
                return Err("set_fill: erode must be a number or null".into());
            }
        }
        if let Some(v) = args
            .get("rho_min")
            .or_else(|| args.get("density_min"))
            .and_then(Value::as_f64)
        {
            f.rho_min = v;
        }
        if let Some(v) = args
            .get("rho_max")
            .or_else(|| args.get("density_max"))
            .and_then(Value::as_f64)
        {
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
            f.protect =
                serde_json::from_value(p.clone()).map_err(|e| format!("set_fill: protect: {e}"))?;
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
        } else if args.get("theta").is_some() || args.get("phi").is_some() {
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
        if let Some(pl) = args.get("plasticity") {
            d.solve.plasticity = if pl.is_null() {
                None
            } else {
                match pl.as_str().unwrap_or("") {
                    "" | "linear" | "none" => None,
                    "j2" => Some(PlasticityLaw::J2),
                    other => {
                        return Err(format!(
                            "set_build: unknown plasticity {other:?} (expected linear|j2)"
                        ))
                    }
                }
            };
        }
        if let Some(hn) = args.get("hardening_n") {
            d.solve.hardening_n =
                if hn.is_null() {
                    None
                } else {
                    Some(hn.as_f64().ok_or_else(|| {
                        "set_build: hardening_n must be a number or null".to_string()
                    })?)
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
        if let Some(ref rurl) = self.remote_compute_url {
            if let Ok((code, val, latency_ms)) =
                http::http_request_json(rurl, "POST", "/api/solve", Some(&req.to_string()), 60_000)
            {
                if code == 200 {
                    let mut out = val;
                    if let Some(obj) = out.as_object_mut() {
                        obj.insert("offloaded_to".to_string(), json!(rurl));
                        obj.insert("remote_roundtrip_ms".to_string(), json!(latency_ms));
                    }
                    return Ok(out);
                }
            }
        }
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

    fn verify(&mut self, args: Value) -> Result<Value, String> {
        let mut q = args;
        q["kind"] = json!("verify");
        self.solve(q)
    }

    fn sensitivity(&mut self, args: Value) -> Result<Value, String> {
        let mut q = args;
        q["kind"] = json!("sensitivity");
        self.solve(q)
    }

    fn optimise(&mut self, args: Value) -> Result<Value, String> {
        let apply = args.get("apply").and_then(Value::as_bool).unwrap_or(true);
        let mut q = args;
        if let Some(obj) = q.as_object_mut() {
            obj.remove("apply");
        }
        q["kind"] = json!("optimise");
        let env = self.solve(q)?;
        if apply {
            if let Some(fsm_str) = env
                .pointer("/optimisation/optimised_fsm")
                .and_then(Value::as_str)
            {
                let d = fsm::parse(fsm_str).map_err(|e| e.to_string())?;
                self.doc = Some(d);
            }
        }
        Ok(env)
    }

    fn analyze_sdf_channel(&mut self, args: Value) -> Result<Value, String> {
        let mut spec: ChannelFlowSpec = serde_json::from_value(args.clone()).unwrap_or_default();
        if let Some(ax) = args.get("axis").and_then(Value::as_str) {
            spec.axis = ChannelAxis::parse(ax)?;
        }
        if let Some(pm) = args.get("passage_mode").and_then(Value::as_str) {
            spec.passage_mode = PassageMode::parse(pm)?;
        }
        let report = {
            let d = self.doc()?;
            run_sdf_channel(d, &spec)?
        };
        let mut bound_region: Option<String> = None;
        if let Some(reg) = args.get("bind_region").and_then(Value::as_str) {
            if !reg.trim().is_empty() {
                let thermal_case = args
                    .get("thermal_case")
                    .and_then(Value::as_str)
                    .unwrap_or("Channel Thermal");
                let pressure_case = args.get("pressure_case").and_then(Value::as_str);
                let d = self.doc.as_mut().ok_or(NO_DOC)?;
                bind_channel_bcs_to_region(d, &report, reg, thermal_case, pressure_case)?;
                bound_region = Some(reg.to_string());
            }
        }
        Ok(json!({
            "report": report,
            "bound_region": bound_region,
        }))
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
            return Err(format!(
                "upsert_shape: {id:?} is not a valid shape identifier"
            ));
        }

        let op = match args.get("op").and_then(Value::as_str).unwrap_or("add") {
            "add" => Op::Add,
            "subtract" => Op::Subtract,
            "intersect" => Op::Intersect,
            other => {
                return Err(format!(
                    "upsert_shape: unknown op {other:?} (expected add|subtract|intersect)"
                ))
            }
        };
        let blend = args.get("blend").and_then(Value::as_f64).unwrap_or(0.0);
        let suppressed = flag(&args, "suppressed");
        let translate: [f64; 3] = match args.get("at") {
            Some(v) => {
                serde_json::from_value(v.clone()).map_err(|e| format!("upsert_shape: at: {e}"))?
            }
            None => [0.0, 0.0, 0.0],
        };
        let rotate: [f64; 3] = match args.get("rotate") {
            Some(v) => serde_json::from_value(v.clone())
                .map_err(|e| format!("upsert_shape: rotate: {e}"))?,
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
            "extrude" => {
                let height = number_field("height", &args)?;
                let profile = parse_profile_vertices(args.get("profile"))?;
                NodeKind::Extrude { profile, height }
            }
            "revolve" => {
                let angle = args.get("angle").and_then(Value::as_f64).unwrap_or(360.0);
                let profile = parse_profile_vertices(args.get("profile"))?;
                NodeKind::Revolve { profile, angle }
            }
            other => {
                return Err(format!(
                    "upsert_shape: unknown shape kind {other:?} (expected box|cylinder|sphere|capsule|torus|extrude|revolve)"
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

    /// Insert an engineering fixing (`self_tapping_screw`, `hex_head_screw`, `hex_cap_bolt`,
    /// `washer`, `hex_nut`, `nyloc_nut`, or `hook_clip_box`) from the built-in catalogue into
    /// the open document, automatically drilling/tapping/counterboring the fixed host object
    /// and computing the minimal (`T_min`) and maximal (`T_max`) torque envelope.
    fn insert_fixing(&mut self, args: Value) -> Result<Value, String> {
        let kind_str = args
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("self_tapping_screw");
        let kind = FixingKind::parse(kind_str).ok_or_else(|| {
            format!(
                "insert_fixing: unknown kind {kind_str:?} (expected self_tapping_screw|hex_head_screw|hex_cap_bolt|washer|hex_nut|nyloc_nut|hook_clip_box)"
            )
        })?;
        let req = FixingInsertRequest {
            kind,
            size: args
                .get("size")
                .and_then(Value::as_str)
                .unwrap_or("M4")
                .to_string(),
            engagement_mm: args
                .get("engagement_mm")
                .and_then(Value::as_f64)
                .unwrap_or(8.0),
            at: args
                .get("at")
                .and_then(|v| serde_json::from_value(v.clone()).ok()),
            id_prefix: args
                .get("id_prefix")
                .and_then(Value::as_str)
                .map(str::to_string),
            host_id: args
                .get("host_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            fastener_material: args
                .get("fastener_material")
                .and_then(Value::as_str)
                .unwrap_or("ti64")
                .to_string(),
            substrate_material: args
                .get("substrate_material")
                .and_then(Value::as_str)
                .map(str::to_string),
            vibration_g: args
                .get("vibration_g")
                .and_then(Value::as_f64)
                .unwrap_or(12.0),
            payload_mass_kg: args
                .get("payload_mass_kg")
                .and_then(Value::as_f64)
                .unwrap_or(0.15),
        };
        if self.doc.is_none() {
            self.doc = Some(Document::default());
        }
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let summary = insert_fixing_into_document(d, &req);
        let problems = d.validate();
        Ok(json!({
            "fixing": summary,
            "shapes": d.root.len(),
            "regions": d.regions.len(),
            "loads": d.loads.len(),
            "problems": problems,
        }))
    }

    /// Solve a 2D parametric sketch (points, segments, circles, 12 geometric + 6 dimensional
    /// constraints, or declarative macros) using damped Levenberg–Marquardt + homotopy continuation
    /// and column-pivoted QR rank decomposition. Returns the solved `(x, y, bulge)` outer & hole
    /// loops, exact DOF count, per-point freedom status, and redundant/conflicting constraint IDs.
    fn solve_sketch(&mut self, args: Value) -> Result<Value, String> {
        let mut sk: ParametricSketch = if let Some(preset_name) =
            args.get("preset").and_then(Value::as_str)
        {
            let mut p = preset_sketch_template(preset_name)?;
            if let Some(sub) = args.get("sketch") {
                let extra: ParametricSketch = serde_json::from_value(sub.clone())
                    .map_err(|e| format!("solve_sketch: sketch: {e}"))?;
                for c in extra.constraints {
                    if let Some(ex) = p.constraints.iter_mut().find(|ec| ec.id == c.id) {
                        *ex = c;
                    } else {
                        p.constraints.push(c);
                    }
                }
            }
            p
        } else if let Some(sub) = args.get("sketch") {
            serde_json::from_value(sub.clone()).map_err(|e| format!("solve_sketch: sketch: {e}"))?
        } else {
            serde_json::from_value(args.clone()).map_err(|e| format!("solve_sketch: {e}"))?
        };

        let mut lets: Vec<(String, f64)> = self
            .doc
            .as_ref()
            .map(|d| d.lets.iter().map(|l| (l.name.clone(), l.value)).collect())
            .unwrap_or_default();
        if let Some(lets_obj) = args.get("lets").and_then(Value::as_object) {
            for (k, v) in lets_obj {
                if let Some(num) = v.as_f64() {
                    if let Some(ex) = lets.iter_mut().find(|(n, _)| n == k) {
                        ex.1 = num;
                    } else {
                        lets.push((k.clone(), num));
                    }
                }
            }
        }

        let report = solve_sketch_doc(&mut sk, &lets)?;
        serde_json::to_value(report).map_err(|e| e.to_string())
    }

    /// Persist and solve a named 2D parametric sketch in `doc.sketches` (creating or updating by `id`)
    /// and re-synchronize any 3D `sketchfeature` bindings referencing that sketch.
    fn upsert_sketch(&mut self, args: Value) -> Result<Value, String> {
        let sketch: ParametricSketch = if let Some(sub) = args.get("sketch") {
            serde_json::from_value(sub.clone())
                .map_err(|e| format!("upsert_sketch: sketch: {e}"))?
        } else {
            serde_json::from_value(args).map_err(|e| format!("upsert_sketch: {e}"))?
        };
        if self.doc.is_none() {
            self.doc = Some(Document::default());
        }
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let outcome = apply_doc_command(d, DocCommand::UpsertSketch { sketch })?;
        Ok(json!({
            "id": outcome.entity_id,
            "action": outcome.action,
            "report": outcome.sketch_report,
            "sketches": d.sketches.len(),
            "problems": outcome.problems,
        }))
    }

    /// Execute any typed [`DocCommand`] mutation against the open document using the shared
    /// authoritative reducer (`fieldsmith_doc::command::apply_doc_command`).
    fn exec_doc_command(&mut self, args: Value) -> Result<Value, String> {
        let cmd: DocCommand = if let Some(sub) = args.get("command") {
            serde_json::from_value(sub.clone()).map_err(|e| format!("apply_doc_command: {e}"))?
        } else {
            serde_json::from_value(args).map_err(|e| format!("apply_doc_command: {e}"))?
        };
        if self.doc.is_none() {
            self.doc = Some(Document::default());
        }
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let outcome = apply_doc_command(d, cmd)?;
        serde_json::to_value(outcome).map_err(|e| e.to_string())
    }

    /// Solve a 2D parametric sketch on a datum plane (`"xy"`, `"xz"`, `"yz"`) or attached directly
    /// to a 3D shape face (`on: "<shape_id>.<face>"`) and compile it into a 3D feature
    /// (`boss_extrude`, `pocket_cut`, `revolve_boss`, or `revolve_cut`) in the open `.fsm` document.
    fn apply_sketch_feature(&mut self, args: Value) -> Result<Value, String> {
        let request: SketchFeatureRequest =
            serde_json::from_value(args).map_err(|e| format!("apply_sketch_feature: {e}"))?;
        if self.doc.is_none() {
            self.doc = Some(Document::default());
        }
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let outcome = apply_doc_command(d, DocCommand::ApplySketchFeature { request })?;
        Ok(json!({
            "feature": outcome.sketch_feature,
            "shapes": d.root.len(),
            "sketches": d.sketches.len(),
            "sketch_features": d.sketch_features.len(),
            "problems": outcome.problems,
            "fsm": fsm::print(d),
        }))
    }

    /// Delete a shape, sketch, region, support, load, temperature, convection, or heatflux by `id`.
    fn delete_entity(&mut self, args: Value) -> Result<Value, String> {
        let id = text_field("id", &args)?;
        let d = self.doc.as_mut().ok_or(NO_DOC)?;
        let outcome = apply_doc_command(d, DocCommand::DeleteEntity { id })?;
        Ok(json!({
            "deleted": outcome.entity_id,
            "kind": outcome.entity_kind,
            "problems": outcome.problems,
        }))
    }

    /// List all registered pluggable [`CoupledEngineeringSystem`] domain packs, their design levers,
    /// epistemic uncertainties, engineering objectives, and reference scenarios.
    fn list_coupled_systems(&mut self, _args: Value) -> Result<Value, String> {
        let case = CffCase::uk5_reference()?;
        let bath_sys = FloatBathCoaterDesignSystem::new(case, 10);
        let nozzle_sys = RegenerativeCooledNozzleSystem::default();
        let candle_sys = SolarCandleIgnitionSystem;

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

        let candle_scenarios = candle_sys.named_scenarios();
        let fixing_sys = FastenerFixingSystem::new();
        let fixing_scenarios = vec![
            NamedScenario {
                name: "A. Plastic Boss Setting (M4 Self-Tapping in PETG, 1.45 N·m)".to_string(),
                description: "M4 Plastite self-tapping screw in ⌀10 mm PETG boss (E = 1.65 GPa, 12 g vibe) — governed by viscoplastic creep & boss hoop stress".to_string(),
                design_p: vec![1.45, 4.0, 0.70, 8.0, 10.0, 1.65, 12.0, -20.0],
            },
            NamedScenario {
                name: "B. Metal Housing Setting (M4 in AlSi10Mg LPBF, 3.20 N·m)".to_string(),
                description: "M4 screw in AlSi10Mg housing (E = 70 GPa, 18 g vibe) — higher T_max governed by thread shear & core tension-torsion yield".to_string(),
                design_p: vec![3.20, 4.0, 0.70, 8.0, 12.0, 70.0, 18.0, -40.0],
            },
            NamedScenario {
                name: "C. JWST Cryo-Optics Setting (M5 Ti64 in SiC Bench @ 38 K, 28 g Launch)".to_string(),
                description: "JWST-class M5 Ti-6Al-4V fastener on SiC optical bench (E = 410 GPa, ΔT = −255 °C cryo cooldown, 28 g Ariane 5 launch)".to_string(),
                design_p: vec![4.80, 5.0, 0.80, 12.0, 15.0, 410.0, 28.0, -255.0],
            },
        ];

        let drone_sys = DeadCatGeminiAntennaSystem::new();
        let drone_scenarios = vec![
            NamedScenario {
                name: "A. Unoptimised Tucked Pod (6.5 mm CFRP Standoff, Free-Space Lengths, 38° Skew)".to_string(),
                description: "Gemini V-antenna tucked flush against rear CFRP standoffs (d_cfrp = 6.5 mm, z = 4 mm) inside a thick 3.2 mm TPU boot — severe dielectric detuning, 42% CFRP eddy loss, and -14 dBi nose-on shadow null".to_string(),
                design_p: vec![6.5, 156.0, 59.0, 38.0, 4.0, 3.2, 0.0, 0.0],
            },
            NamedScenario {
                name: "B. Optimised Rear V-Tail Pod (24 mm Standoff, 88° Orthogonal Skew, L/C Matched)".to_string(),
                description: "Dielectric pre-compensated dipole lengths (136.5 / 51.2 mm) on a 24 mm elevated TPU boom (z = 16 mm, α = 88°) with L-network match — S11 < -18 dB in both bands and > 82% efficiency".to_string(),
                design_p: vec![24.0, 136.5, 51.2, 88.0, 16.0, 1.6, 1.8, 0.65],
            },
        ];

        let swirl_sys = AnnularSwirlSprayCombustorSystem::default();
        let swirl_scenarios = swirl_sys.named_scenarios();

        let mut systems = Vec::with_capacity(7);
        if let Some(ref doc) = self.doc {
            if doc.system_block.is_some() {
                let decl_sys = DeclarativeCoupledSystem::from_document(doc)?;
                systems.push(json!({
                    "system_id": decl_sys.system_id(),
                    "system_title": decl_sys.system_title(),
                    "industry_sector": decl_sys.industry_sector(),
                    "num_dofs": decl_sys.num_dofs(),
                    "fields": decl_sys.field_descriptors(),
                    "design_parameters": decl_sys.design_parameters(),
                    "epistemic_parameters": decl_sys.epistemic_parameters(),
                    "objectives": decl_sys.objective_descriptors(),
                    "scenarios": decl_sys.named_scenarios(),
                    "is_declarative_doc_system": true,
                }));
            }
        }
        systems.extend([
            json!({
                "system_id": bath_sys.system_id(),
                "system_title": bath_sys.system_title(),
                "industry_sector": bath_sys.industry_sector(),
                "num_dofs": bath_sys.num_dofs(),
                "fields": bath_sys.field_descriptors(),
                "design_parameters": bath_sys.design_parameters(),
                "epistemic_parameters": bath_sys.epistemic_parameters(),
                "objectives": bath_sys.objective_descriptors(),
                "scenarios": bath_scenarios,
            }),
            json!({
                "system_id": nozzle_sys.system_id(),
                "system_title": nozzle_sys.system_title(),
                "industry_sector": nozzle_sys.industry_sector(),
                "num_dofs": nozzle_sys.num_dofs(),
                "fields": nozzle_sys.field_descriptors(),
                "design_parameters": nozzle_sys.design_parameters(),
                "epistemic_parameters": nozzle_sys.epistemic_parameters(),
                "objectives": nozzle_sys.objective_descriptors(),
                "scenarios": nozzle_scenarios,
            }),
            json!({
                "system_id": candle_sys.system_id(),
                "system_title": candle_sys.system_title(),
                "industry_sector": candle_sys.industry_sector(),
                "num_dofs": candle_sys.num_dofs(),
                "fields": candle_sys.field_descriptors(),
                "design_parameters": candle_sys.design_parameters(),
                "epistemic_parameters": candle_sys.epistemic_parameters(),
                "objectives": candle_sys.objective_descriptors(),
                "scenarios": candle_scenarios,
            }),
            json!({
                "system_id": fixing_sys.system_id(),
                "system_title": fixing_sys.system_title(),
                "industry_sector": fixing_sys.industry_sector(),
                "num_dofs": fixing_sys.num_dofs(),
                "fields": fixing_sys.field_descriptors(),
                "design_parameters": fixing_sys.design_parameters(),
                "epistemic_parameters": fixing_sys.epistemic_parameters(),
                "objectives": fixing_sys.objective_descriptors(),
                "scenarios": fixing_scenarios,
            }),
            json!({
                "system_id": drone_sys.system_id(),
                "system_title": drone_sys.system_title(),
                "industry_sector": drone_sys.industry_sector(),
                "num_dofs": drone_sys.num_dofs(),
                "fields": drone_sys.field_descriptors(),
                "design_parameters": drone_sys.design_parameters(),
                "epistemic_parameters": drone_sys.epistemic_parameters(),
                "objectives": drone_sys.objective_descriptors(),
                "scenarios": drone_scenarios,
            }),
            json!({
                "system_id": swirl_sys.system_id(),
                "system_title": swirl_sys.system_title(),
                "industry_sector": swirl_sys.industry_sector(),
                "num_dofs": swirl_sys.num_dofs(),
                "fields": swirl_sys.field_descriptors(),
                "design_parameters": swirl_sys.design_parameters(),
                "epistemic_parameters": swirl_sys.epistemic_parameters(),
                "objectives": swirl_sys.objective_descriptors(),
                "scenarios": swirl_scenarios,
            }),
        ]);

        Ok(json!({ "systems": systems }))
    }

    /// Solve a registered [`CoupledEngineeringSystem`] at `design_p` and `epistemic_theta`,
    /// returning the converged monolithic state, Sparse Adjoint sensitivities `dΦ_m/dp_k`,
    /// `p10/p50/p90` epistemic intervals, and 3D `.fsm` CAD twin (automatically loaded into
    /// the open session document unless `sync_cad: false` is passed).
    fn solve_coupled_system(&mut self, args: Value) -> Result<Value, String> {
        let doc_decl_id = self
            .doc
            .as_ref()
            .and_then(|d| d.system_block.as_ref().map(|s| s.id.clone()));
        let doc_coupled_id = self.doc.as_ref().and_then(|d| d.coupled_system.clone());
        let system_id = args
            .get("system_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| doc_decl_id.clone())
            .or(doc_coupled_id)
            .or_else(|| self.coupled_system_id.clone())
            .unwrap_or_else(|| "pilkington_float_bath_cvd_coater_v1".to_string());
        let sync_cad = args
            .get("sync_cad")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let req_p: Option<Vec<f64>> = args
            .get("design_p")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .or_else(|| self.design_p.clone());
        let req_theta: Option<Vec<f64>> = args
            .get("epistemic_theta")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .or_else(|| self.epistemic_theta.clone());

        if doc_decl_id.as_deref() == Some(system_id.as_str()) {
            let doc_ref = self.doc.as_ref().expect("checked doc_decl_id");
            let sys = DeclarativeCoupledSystem::from_document(doc_ref)?;
            let p = req_p
                .filter(|v| v.len() == sys.design_parameters().len())
                .unwrap_or_else(|| sys.nominal_design_vector());
            let theta = req_theta
                .filter(|v| v.len() == sys.epistemic_parameters().len())
                .unwrap_or_else(|| sys.nominal_epistemic_vector());
            let cfg = sys.engine_config();
            let analysis = MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p, &theta)?;
            let obj_descs = sys.objective_descriptors();
            let pass_p50 = analysis.all_constraints_satisfied_p50(&obj_descs);
            let pass_p90 = analysis.all_constraints_satisfied_robust_p90(&obj_descs);
            let declarative_visual = sys.extract_visual_fields(&analysis.state, &p, &theta);
            let synced_doc = sys.synced_document(&analysis.state, &p, &theta)?;
            let fsm_3d = fsm::print(&synced_doc);
            if sync_cad {
                self.doc = Some(synced_doc);
            }
            self.coupled_system_id = Some(system_id);
            self.design_p = Some(p);
            self.epistemic_theta = Some(theta);
            return Ok(json!({
                "analysis": analysis,
                "constraints_pass_p50": pass_p50,
                "constraints_pass_robust_p90": pass_p90,
                "declarative_visual": declarative_visual,
                "synced_to_cad_doc": sync_cad,
                "fsm_3d": fsm_3d,
            }));
        }

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
            let fsm_3d = sys.build_3d_fsm_from_state(&analysis.state, &p, &theta);
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
            let fsm_3d = sys.build_3d_fsm_from_state(&analysis.state, &p, &theta);
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
        } else if system_id == "solar_candle_ignition_v1" {
            let sys = SolarCandleIgnitionSystem;
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
            let candle_flame_visual = sys.extract_visual_fields(&analysis.state, &p, &theta);
            let fsm_3d = sys.build_3d_fsm_from_state(&analysis.state, &p, &theta);
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
                "candle_flame_visual": candle_flame_visual,
                "synced_to_cad_doc": sync_cad && fsm_3d.is_some(),
                "fsm_3d": fsm_3d,
            }))
        } else if system_id == "jwst_fastener_fixing_envelope_v1" {
            let sys = FastenerFixingSystem::new();
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
            let fsm_3d = sys.build_3d_fsm_from_state(&analysis.state, &p, &theta);
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
        } else if system_id == "deadcat_4in_gemini_antenna_v1" {
            let sys = DeadCatGeminiAntennaSystem::new();
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
            let spec = sys.spec_from_params(&p, &theta);
            let vna_report = analyze_drone_antenna(&spec);
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
                "drone_antenna_vna": vna_report,
                "synced_to_cad_doc": sync_cad && fsm_3d.is_some(),
                "fsm_3d": fsm_3d,
            }))
        } else if system_id == "annular_swirl_spray_combustor_v1" {
            use fieldsmith_solve::coupled_system::{
                solve_annular_swirl_spray_combustor, SwirlSprayCombustorParams,
            };
            let sys = AnnularSwirlSprayCombustorSystem::default();
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
            let swirl_report = solve_annular_swirl_spray_combustor(
                &SwirlSprayCombustorParams::from_vectors(&p, &theta),
            );
            let fsm_3d = sys.build_3d_fsm_from_state(&analysis.state, &p, &theta);
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
                "swirl_combustor": swirl_report,
                "synced_to_cad_doc": sync_cad && fsm_3d.is_some(),
                "fsm_3d": fsm_3d,
            }))
        } else {
            Err(format!(
                "solve_coupled_system: unknown system_id {system_id:?}"
            ))
        }
    }

    /// Solve the 4-inch DeadCat FPV Drone + Gemini Dual-Band (`900 MHz` + `2.4 GHz`) Antenna
    /// VNA sweep (`S11`, `S21`, `VSWR`, Smith Chart `Gamma(f)`, radiation efficiency `eta_rad`)
    /// and 3D radiation lobes (`G(theta, phi)`), optionally syncing the 3D `.fsm` drone assembly.
    fn solve_drone_antenna(&mut self, args: Value) -> Result<Value, String> {
        let sync_cad = args
            .get("sync_cad")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let spec = parse_drone_antenna_spec(&args);
        let report = analyze_drone_antenna(&spec);
        let fsm_3d = build_deadcat_drone_fsm(&spec);
        if sync_cad {
            let d = fsm::parse(&fsm_3d).map_err(|e| e.to_string())?;
            self.doc = Some(d);
        }
        Ok(json!({
            "report": report,
            "synced_to_cad_doc": sync_cad,
            "fsm_3d": fsm_3d,
        }))
    }

    /// Optimise the 4-inch DeadCat FPV Drone + Gemini Dual-Band Antenna geometry (`cfrp_standoff_mm`,
    /// `elevation_above_plate_mm`, `low_band_arm_length_mm`, `high_band_arm_length_mm`,
    /// `diversity_skew_deg`, `dielectric_pod_wall_mm`) and L-network impedance matching, and sync
    /// the optimised 3D `.fsm` drone assembly into the session document.
    fn optimise_drone_antenna(&mut self, args: Value) -> Result<Value, String> {
        let sync_cad = args
            .get("sync_cad")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let max_iters = args
            .get("iterations")
            .and_then(Value::as_u64)
            .unwrap_or(8)
            .clamp(1, 25) as usize;
        let spec = if args.get("preset").and_then(Value::as_str) == Some("unoptimised_tucked")
            || args.as_object().map(|o| o.is_empty()).unwrap_or(true)
        {
            DroneAntennaDesignSpec::unoptimised_tucked_preset()
        } else {
            parse_drone_antenna_spec(&args)
        };
        let opt = optimise_drone_antenna(&spec, max_iters);
        if sync_cad {
            let d = fsm::parse(&opt.fsm_3d).map_err(|e| e.to_string())?;
            self.doc = Some(d);
        }
        Ok(json!({
            "initial_report": opt.initial_report,
            "optimised_report": opt.optimised_report,
            "iterations": opt.iterations,
            "synced_to_cad_doc": sync_cad,
            "fsm_3d": opt.fsm_3d,
        }))
    }

    /// Take `steps` (default `1`) automatic Sparse Adjoint `-dΦ/dp` design optimisation steps
    /// on a [`CoupledEngineeringSystem`], updating `design_p` and synthesising the updated
    /// 3D `.fsm` CAD twin into the session document.
    fn optimise_coupled_system(&mut self, args: Value) -> Result<Value, String> {
        let doc_decl_id = self
            .doc
            .as_ref()
            .and_then(|d| d.system_block.as_ref().map(|s| s.id.clone()));
        let doc_coupled_id = self.doc.as_ref().and_then(|d| d.coupled_system.clone());
        let system_id = args
            .get("system_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| doc_decl_id.clone())
            .or(doc_coupled_id)
            .or_else(|| self.coupled_system_id.clone())
            .unwrap_or_else(|| "pilkington_float_bath_cvd_coater_v1".to_string());
        let steps = args
            .get("steps")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .clamp(1, 20) as usize;
        let sync_cad = args
            .get("sync_cad")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let req_p: Option<Vec<f64>> = args
            .get("design_p")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .or_else(|| self.design_p.clone());
        let req_theta: Option<Vec<f64>> = args
            .get("epistemic_theta")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .or_else(|| self.epistemic_theta.clone());

        if doc_decl_id.as_deref() == Some(system_id.as_str()) {
            let doc_clone = self.doc.clone().expect("checked doc_decl_id");
            let sys = DeclarativeCoupledSystem::from_document(&doc_clone)?;
            let cfg = sys.engine_config();
            return self
                .run_adjoint_optimisation_loop(&sys, &cfg, req_p, req_theta, steps, sync_cad);
        }

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
        } else if system_id == "solar_candle_ignition_v1" {
            let sys = SolarCandleIgnitionSystem;
            let cfg = EngineConfig::default();
            self.run_adjoint_optimisation_loop(&sys, &cfg, req_p, req_theta, steps, sync_cad)
        } else if system_id == "jwst_fastener_fixing_envelope_v1" {
            let sys = FastenerFixingSystem::new();
            let cfg = EngineConfig::default();
            self.run_adjoint_optimisation_loop(&sys, &cfg, req_p, req_theta, steps, sync_cad)
        } else if system_id == "deadcat_4in_gemini_antenna_v1" {
            let sys = DeadCatGeminiAntennaSystem::new();
            let cfg = EngineConfig::default();
            self.run_adjoint_optimisation_loop(&sys, &cfg, req_p, req_theta, steps, sync_cad)
        } else if system_id == "annular_swirl_spray_combustor_v1" {
            let sys = AnnularSwirlSprayCombustorSystem::default();
            let cfg = EngineConfig::default();
            self.run_adjoint_optimisation_loop(&sys, &cfg, req_p, req_theta, steps, sync_cad)
        } else {
            Err(format!(
                "optimise_coupled_system: unknown system_id {system_id:?}"
            ))
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
        let fsm_3d = sys.build_3d_fsm_from_state(&final_analysis.state, &p, &theta);
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

    /// Discover other Fieldsmith compute servers (`fieldsmith-mcp --serve`) on the local subnet,
    /// loopback, or Tailscale/WireGuard interfaces via UDP broadcast (`:18790`) and parallel
    /// `/api/health` TCP probes. Returns peers sorted by highest thread count and lowest latency.
    fn discover_compute_peers(&mut self, args: Value) -> Result<Value, String> {
        let ports: Vec<u16> = args
            .get("ports")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_u64)
                    .filter_map(|p| u16::try_from(p).ok())
                    .collect()
            })
            .filter(|v: &Vec<u16>| !v.is_empty())
            .unwrap_or_else(|| vec![8080, 18789]);
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(180);
        let extra_hosts: Vec<String> = args
            .get("extra_hosts")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let include_subnet = args
            .get("include_subnet_sweep")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let peers = http::discover_peers(&ports, timeout_ms, &extra_hosts, include_subnet);
        Ok(json!({
            "peers": peers,
            "active_remote_compute_url": self.remote_compute_url,
            "local_hostname": http::local_hostname(),
            "local_threads": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        }))
    }

    /// Connect this Fieldsmith instance to an external Fieldsmith compute server (e.g. a
    /// workstation running `fieldsmith-mcp --serve 0.0.0.0:8080`) by URL or `"auto"` (which
    /// selects the discovered peer with the highest thread count), syncing the current `.fsm`
    /// workspace and routing heavy `solve` calls to the remote peer.
    fn connect_compute_peer(&mut self, args: Value) -> Result<Value, String> {
        let url_arg = args
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("auto")
            .trim();
        if url_arg.is_empty() || url_arg == "none" || url_arg == "disconnect" {
            return self.disconnect_compute_peer(json!({}));
        }
        let target_url = if url_arg == "auto" {
            let extra_hosts: Vec<String> = args
                .get("extra_hosts")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let peers = http::discover_peers(&[8080, 18789], 180, &extra_hosts, true);
            peers.into_iter().next().map(|p| p.url).ok_or_else(|| {
                "connect_compute_peer: no Fieldsmith compute peers discovered".to_string()
            })?
        } else {
            let (_, _, norm) = http::parse_http_host_port(url_arg)?;
            norm
        };
        let (host, port, norm_url) = http::parse_http_host_port(&target_url)?;
        let peer_info = http::probe_peer_health(&host, port, 1500, "connect").ok_or_else(|| {
            format!("connect_compute_peer: failed to reach {norm_url}/api/health")
        })?;
        let sync_payload = json!({
            "actor": http::local_hostname(),
            "last_tool": "connect_compute_peer",
            "summary": format!("Synced workspace from {} to compute peer {}", http::local_hostname(), peer_info.hostname),
            "doc_fsm": self.doc.as_ref().map(fsm::print),
            "coupled_system_id": self.coupled_system_id,
            "design_p": self.design_p,
            "epistemic_theta": self.epistemic_theta,
        });
        let _ = http::http_request_json(
            &norm_url,
            "POST",
            "/api/workspace",
            Some(&sync_payload.to_string()),
            2000,
        );
        self.remote_compute_url = Some(norm_url.clone());
        Ok(json!({
            "status": "connected",
            "remote_compute_url": norm_url,
            "peer": peer_info,
        }))
    }

    /// Disconnect from any attached external Fieldsmith compute server and return to local execution.
    fn disconnect_compute_peer(&mut self, _args: Value) -> Result<Value, String> {
        let prev = self.remote_compute_url.take();
        Ok(json!({
            "status": "disconnected",
            "previous_remote_compute_url": prev,
            "remote_compute_url": null,
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

fn parse_profile_vertices(val: Option<&Value>) -> Result<Vec<ProfileVertex>, String> {
    let arr = val.and_then(Value::as_array).ok_or_else(|| {
        "profile: expected an array of [x, y] or {x, y, bulge} vertices".to_string()
    })?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        if let Some(coords) = item.as_array() {
            if coords.len() < 2 {
                return Err("profile vertex array must have at least [x, y]".into());
            }
            let x = coords[0]
                .as_f64()
                .ok_or("profile vertex x must be a number")?;
            let y = coords[1]
                .as_f64()
                .ok_or("profile vertex y must be a number")?;
            let bulge = coords.get(2).and_then(Value::as_f64).unwrap_or(0.0);
            out.push(ProfileVertex { x, y, bulge });
        } else if let Some(obj) = item.as_object() {
            let x = obj
                .get("x")
                .and_then(Value::as_f64)
                .ok_or("profile vertex missing numeric 'x'")?;
            let y = obj
                .get("y")
                .and_then(Value::as_f64)
                .ok_or("profile vertex missing numeric 'y'")?;
            let bulge = obj.get("bulge").and_then(Value::as_f64).unwrap_or(0.0);
            out.push(ProfileVertex { x, y, bulge });
        } else {
            return Err("profile vertex must be [x, y, bulge?] or {x, y, bulge?}".into());
        }
    }
    if out.len() < 2 {
        return Err("profile requires at least 2 vertices".into());
    }
    Ok(out)
}

fn parse_drone_antenna_spec(args: &Value) -> DroneAntennaDesignSpec {
    let mut s = if args.get("preset").and_then(Value::as_str) == Some("unoptimised_tucked") {
        DroneAntennaDesignSpec::unoptimised_tucked_preset()
    } else {
        DroneAntennaDesignSpec::default()
    };
    if let Some(t) = args.get("topology").and_then(Value::as_str) {
        s.topology = DroneAntennaTopology::from_str_id(t);
    }
    if let Some(v) = args.get("target_low_freq_ghz").and_then(Value::as_f64) {
        s.target_low_freq_ghz = v;
    }
    if let Some(v) = args.get("target_high_freq_ghz").and_then(Value::as_f64) {
        s.target_high_freq_ghz = v;
    }
    if let Some(v) = args.get("cfrp_standoff_mm").and_then(Value::as_f64) {
        s.cfrp_standoff_mm = v;
    }
    if let Some(v) = args.get("low_band_arm_length_mm").and_then(Value::as_f64) {
        s.low_band_arm_length_mm = v;
    }
    if let Some(v) = args.get("high_band_arm_length_mm").and_then(Value::as_f64) {
        s.high_band_arm_length_mm = v;
    }
    if let Some(v) = args.get("diversity_skew_deg").and_then(Value::as_f64) {
        s.diversity_skew_deg = v;
    }
    if let Some(v) = args.get("elevation_above_plate_mm").and_then(Value::as_f64) {
        s.elevation_above_plate_mm = v;
    }
    if let Some(v) = args.get("dielectric_pod_wall_mm").and_then(Value::as_f64) {
        s.dielectric_pod_wall_mm = v;
    }
    if let Some(v) = args.get("pod_eps_r").and_then(Value::as_f64) {
        s.pod_eps_r = v;
    }
    if let Some(v) = args.get("pod_loss_tan").and_then(Value::as_f64) {
        s.pod_loss_tan = v;
    }
    if let Some(v) = args
        .get("cfrp_conductivity_s_per_m")
        .and_then(Value::as_f64)
    {
        s.cfrp_conductivity_s_per_m = v;
    }
    if let Some(v) = args.get("wheelbase_mm").and_then(Value::as_f64) {
        s.wheelbase_mm = v;
    }
    if let Some(v) = args.get("match_series_nh").and_then(Value::as_f64) {
        s.match_series_nh = v;
    }
    if let Some(v) = args.get("match_shunt_pf").and_then(Value::as_f64) {
        s.match_shunt_pf = v;
    }
    if let Some(v) = args.get("tx_power_mw").and_then(Value::as_f64) {
        s.tx_power_mw = v;
    }
    s
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
 {"name":"open","description":"Open the session's document (.fsm text or base64 .fsmz). Exactly one of doc/docz.","inputSchema":{"type":"object","properties":{"doc":{"type":"string"},"fsm":{"type":"string"},"docz":{"type":"string"}}}},
 {"name":"new_document","description":"Reset the session workspace to a fresh empty .fsm document with optional `name` and `material`.","inputSchema":{"type":"object","properties":{"name":{"type":"string"},"material":{"type":"string"}}}},
 {"name":"inspect","description":"The open document's analysis setup: regions, supports, loads, temperatures, convections, heat fluxes, custom materials, coupled system, asserts, fill, solve settings, validate problems, missing mesh grids.","inputSchema":{"type":"object","additionalProperties":false}},
 {"name":"list_shapes","description":"Inspect the open document's 3D CSG/SDF shape tree (`shapes`), `lets` variables, and `consumed` expressions.","inputSchema":{"type":"object","additionalProperties":false}},
 {"name":"upsert_shape","description":"Create or update a 3D CAD primitive or profile-swept shape (`box`, `cylinder`, `sphere`, `capsule`, `torus`, `extrude`, `revolve`) in the open document's root CSG/SDF tree.","inputSchema":{"type":"object","required":["kind"],"properties":{"id":{"type":"string"},"kind":{"type":"string","enum":["box","cylinder","sphere","capsule","torus","extrude","revolve"]},"op":{"type":"string","enum":["add","subtract","intersect"]},"blend":{"type":"number"},"at":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"rotate":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"size":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"radius":{"type":"number"},"height":{"type":"number"},"length":{"type":"number"},"major":{"type":"number"},"minor":{"type":"number"},"round":{"type":"number"},"angle":{"type":"number"},"profile":{"type":"array","items":{"type":["object","array"]}},"color":{"type":"string"},"material":{"type":"string"},"suppressed":{"type":"boolean"}}}},
 {"name":"insert_fixing","description":"Insert an engineering fixing (`self_tapping_screw`, `hex_head_screw`, `hex_cap_bolt`, `washer`, `hex_nut`, `nyloc_nut`, or `hook_clip_box`) from the built-in geometry catalogue, automatically drilling/tapping/counterboring the fixed object and calculating minimal (`T_min`) and maximal (`T_max`) torque.","inputSchema":{"type":"object","required":["kind"],"properties":{"kind":{"type":"string","enum":["self_tapping_screw","hex_head_screw","hex_cap_bolt","washer","hex_nut","nyloc_nut","hook_clip_box"]},"size":{"type":"string","enum":["M2.5","M3","M4","M5","M6","M8"]},"engagement_mm":{"type":"number"},"at":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"id_prefix":{"type":"string"},"host_id":{"type":"string"},"fastener_material":{"type":"string"},"substrate_material":{"type":"string"},"vibration_g":{"type":"number"},"payload_mass_kg":{"type":"number"}}}},
 {"name":"solve_sketch","description":"Solve a 2D parametric sketch (`points`, `segments`, `circles`, 12 geometric + 6 dimensional `constraints`, or high-level `macros`: `rect`, `center_rect`, `circle`, `slot`, `polygon`, `bolt_circle`, `fillet_corner`) using damped Levenberg-Marquardt + homotopy continuation and column-pivoted QR rank analysis. Returns solved `(x, y, bulge)` outer/hole profiles, exact `dof`, per-point freedom, and redundant/conflicting constraint diagnostics.","inputSchema":{"type":"object","properties":{"id":{"type":"string"},"preset":{"type":"string","enum":["flanged_mounting_bracket","turned_aerospace_spool","slotted_adjustment_link","bolt_flange_plate"]},"sketch":{"type":"object"},"points":{"type":"array","items":{"type":"object"}},"segments":{"type":"array","items":{"type":"object"}},"circles":{"type":"array","items":{"type":"object"}},"constraints":{"type":"array","items":{"type":"object"}},"macros":{"type":"array","items":{"type":"object"}},"lets":{"type":"object"}}}},
 {"name":"upsert_sketch","description":"Create or update a persistent 2D parametric sketch in the open `.fsm` document (`doc.sketches`), solve its constraints against current `let` variables, and re-synchronize any bound `sketchfeature` 3D operations.","inputSchema":{"type":"object","properties":{"sketch":{"type":"object"},"id":{"type":"string"},"plane":{"type":"object"},"points":{"type":"array","items":{"type":"object"}},"segments":{"type":"array","items":{"type":"object"}},"circles":{"type":"array","items":{"type":"object"}},"constraints":{"type":"array","items":{"type":"object"}},"macros":{"type":"array","items":{"type":"object"}}}}},
 {"name":"apply_sketch_feature","description":"Solve a 2D parametric sketch on a world plane (`xy`, `xz`, `yz`) or attached directly to a 3D shape's face (`on: \"<shape_id>.<face>\"`), persist both `sketch` and `sketchfeature` in the `.fsm` document, and compile/update the 3D feature (`boss_extrude`, `pocket_cut`, `revolve_boss`, or `revolve_cut`).","inputSchema":{"type":"object","required":["feature_id","operation"],"properties":{"feature_id":{"type":"string"},"sketch_id":{"type":"string"},"associate_with_host":{"type":"boolean"},"preset":{"type":"string","enum":["flanged_mounting_bracket","turned_aerospace_spool","slotted_adjustment_link","bolt_flange_plate"]},"on":{"type":"string"},"sketch":{"type":"object"},"operation":{"type":"object","required":["kind"],"properties":{"kind":{"type":"string","enum":["boss_extrude","pocket_cut","revolve_boss","revolve_cut"]},"height":{"type":"number"},"depth":{"type":"number"},"through_all":{"type":"boolean"},"angle_deg":{"type":"number"},"blend":{"type":"number"},"symmetric":{"type":"boolean"}}},"lets":{"type":"object"}}}},
 {"name":"apply_doc_command","description":"Apply a typed `DocCommand` (`set_material`, `upsert_custom_material`, `set_let`, `delete_let`, `upsert_node`, `delete_entity`, `upsert_region`, `upsert_support`, `upsert_load`, `upsert_temperature`, `upsert_convection`, `upsert_heat_flux`, `set_fill`, `set_solve`, `upsert_sketch`, `delete_sketch`, `apply_sketch_feature`, `set_system_block`, `upsert_assert`, `delete_assert`) via the shared authoritative reducer.","inputSchema":{"type":"object","required":["cmd"],"properties":{"cmd":{"type":"string"}}}},
 {"name":"delete_entity","description":"Delete a shape, sketch, region, support, load, temperature, convection, or heatflux by `id`.","inputSchema":{"type":"object","required":["id"],"properties":{"id":{"type":"string"}}}},
 {"name":"add_region","description":"Add a named region (union of shape-face selectors).","inputSchema":{"type":"object","required":["selectors"],"properties":{"id":{"type":"string"},"selectors":{"type":"array","items":{"type":"object","required":["shape","face"],"properties":{"shape":{"type":"string"},"face":{"type":"string"},"patch":{"type":"integer"}}}}}}},
 {"name":"add_support","description":"Fix displacement components on a region.","inputSchema":{"type":"object","required":["region","fixed"],"properties":{"id":{"type":"string"},"region":{"type":"string"},"fixed":{"type":"array","items":{"type":"boolean"},"minItems":3,"maxItems":3}}}},
 {"name":"add_load","description":"Apply a load (force/pressure/preload/distributed, including spatially varying `pressure_fn` f(x,y,z,r)) on a region for a load case.","inputSchema":{"type":"object","required":["region"],"properties":{"id":{"type":"string"},"region":{"type":"string"},"case":{"type":"string"},"kind":{"type":"object"},"force":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"pressure":{"type":"number"},"pressure_fn":{"type":"string"},"preload":{"type":"number"},"axis":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3}}}},
 {"name":"add_temperature","description":"Apply a fixed or spatially varying (`temp_fn` f(x,y,z,r)) boundary temperature on a region for a thermal case.","inputSchema":{"type":"object","required":["region"],"properties":{"id":{"type":"string"},"region":{"type":"string"},"celsius":{"type":"number"},"temp_fn":{"type":"string"},"case":{"type":"string"}}}},
 {"name":"add_convection","description":"Apply a Robin convective boundary condition (`h` in 1/mm = `(h_SI / k_solid) * 1e-3`, `t_inf` in °C, with optional world-space `h_fn` and `tinf_fn` f(x,y,z,r)) on a region for a thermal case.","inputSchema":{"type":"object","required":["region"],"properties":{"id":{"type":"string"},"region":{"type":"string"},"h":{"type":"number"},"h_fn":{"type":"string"},"t_inf":{"type":"number"},"tinf":{"type":"number"},"tinf_fn":{"type":"string"},"case":{"type":"string"}}}},
 {"name":"add_heatflux","description":"Apply a Neumann heat-flux boundary condition (`q` in K/mm, or world-space `q_fn` f(x,y,z,r)) on a region for a thermal case.","inputSchema":{"type":"object","required":["region"],"properties":{"id":{"type":"string"},"region":{"type":"string"},"q":{"type":"number"},"q_fn":{"type":"string"},"case":{"type":"string"}}}},
 {"name":"set_material","description":"Assign a material label to a specific shape `node`, or omit `node` to set the document's default material.","inputSchema":{"type":"object","required":["material"],"properties":{"node":{"type":"string"},"material":{"type":"string"}}}},
 {"name":"upsert_custom_material","description":"Define or update a typed `custommaterial` in the open document, including Hollomon plasticity (`hardening_n`, `hardening_k`), brittle Weibull modulus (`weibull_m`), and `role` (`\"structural\"` or `\"fluid\"`).","inputSchema":{"type":"object","required":["id","e","nu","density"],"properties":{"id":{"type":"string"},"name":{"type":"string"},"e":{"type":"number"},"nu":{"type":"number"},"yield_strength":{"type":"number"},"yield":{"type":"number"},"density":{"type":"number"},"min_wall":{"type":"number"},"alpha":{"type":"number"},"k":{"type":"number"},"yield_z":{"type":"number"},"shear_z":{"type":"number"},"hardening_n":{"type":"number"},"hardening_k":{"type":"number"},"weibull_m":{"type":"number"},"role":{"type":"string","enum":["structural","fluid"]},"source":{"type":"string"}}}},
 {"name":"set_let","description":"Set a named `let` variable (or dotted parameter address) and re-evaluate dependent expressions.","inputSchema":{"type":"object","required":["name","value"],"properties":{"name":{"type":"string"},"value":{"type":"number"}}}},
 {"name":"set_fill","description":"Configure or update the document's TPMS lattice fill (including `morph`, `cell_min`, `skin_max`, `erode`, and `rotate`), or clear it (`clear: true`).","inputSchema":{"type":"object","properties":{"clear":{"type":"boolean"},"family":{"type":"string","enum":["gyroid","primitive","diamond"]},"morph":{"type":["string","null"],"enum":["gyroid","primitive","diamond",null]},"mode":{"type":"string","enum":["sheet","network"]},"cell":{"type":"number"},"cell_min":{"type":["number","null"]},"skin":{"type":"number"},"skin_max":{"type":["number","null"]},"erode":{"type":["number","null"]},"rho_min":{"type":"number"},"rho_max":{"type":"number"},"curve":{"type":"number"},"min_wall":{"type":"number"},"protect":{"type":"array","items":{"type":"string"}},"margin":{"type":"number"},"case":{"type":["string","null"]},"rotate":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3}}}},
 {"name":"set_build","description":"Set or clear the additive build orientation `solve build=(theta_deg, phi_deg)`, target element `size`, and constitutive `plasticity` (`\"linear\"` or `\"j2\"`) / `hardening_n`.","inputSchema":{"type":"object","properties":{"clear":{"type":"boolean"},"angles":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},"theta":{"type":"number"},"phi":{"type":"number"},"size":{"type":["number","null"]},"plasticity":{"type":["string","null"],"enum":["linear","j2",null]},"hardening_n":{"type":["number","null"]}}}},
 {"name":"upsert_assert","description":"Create or update an executable engineering contract (`assert` statement) in the open `.fsm` document (`metric` such as `max_vm_elem`, `max_displacement`, `compliance`, `max_temp`, `min_safety_factor`, `mass_g`, `mode_1`; `target` such as `\"<= 150\"`, `\">= 1.5\"`, or numeric value with `tolerance`).","inputSchema":{"type":"object","required":["metric","target"],"properties":{"index":{"type":"integer"},"label":{"type":"string"},"metric":{"type":"string"},"target":{"type":["string","number"]},"tolerance":{"type":"string"},"tol":{"type":"string"},"unit":{"type":"string"}}}},
 {"name":"delete_assert","description":"Delete an executable `assert` engineering contract from the open `.fsm` document by `index` or `label`.","inputSchema":{"type":"object","properties":{"index":{"type":"integer"},"label":{"type":"string"}}}},
 {"name":"doc_text","description":"The open document as .fsm text (a print of the current model, edits included).","inputSchema":{"type":"object","additionalProperties":false}},
 {"name":"solve","description":"Run the API question (solve/ladder/modes/sweep/overhang/verify/sensitivity) on the session's document; returns the same JSON envelope the `fieldsmith api` verb writes.","inputSchema":{"type":"object","required":["kind"],"properties":{"kind":{"type":"string","enum":["solve","ladder","modes","sweep","overhang","verify","sensitivity"]},"dofs":{"type":"integer"},"h":{"type":"number"},"n":{"type":"integer"},"rungs":{"type":"array","items":{"type":"integer"}},"tolerance":{"type":"string"},"budget":{"type":"string"},"param":{"type":"string"},"params":{"type":"array","items":{"type":"string"}},"values":{"type":"array","items":{"type":"number"}},"build":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"threshold_deg":{"type":"number"},"equilibrate":{"type":"boolean"}}}},
 {"name":"verify","description":"Evaluate all `assert` / `@assert:` engineering contracts on the open `.fsm` document; returns the `apiv: 2` envelope with `status: \"ok\"` when all pass or `status: \"refusal\"` when any contract fails.","inputSchema":{"type":"object","properties":{"dofs":{"type":"integer"},"h":{"type":"number"}}}},
 {"name":"sensitivity","description":"Compute 3D P2 parameter sensitivities (`dC/dp`, `dm/dp`, `du_max/dp`, `dsigma_vM/dp`, `dT_max/dp`, and dimensionless elasticities) across `params` (or all non-derived `let` / `system` design variables in the open `.fsm` document) using mechanical & staggered thermo-elastic discrete adjoints (`O(1)` solves in parameter count `k`).","inputSchema":{"type":"object","properties":{"params":{"type":"array","items":{"type":"string"}},"dofs":{"type":"integer"},"h":{"type":"number"}}}},
 {"name":"optimise","description":"Contract-constrained 3D continuum & coupled 0D/1D+3D parameter optimiser: minimises `objective` (`mass_g` default, `compliance`, `max_vm_elem`, `max_displacement`, `max_temp`) across `params` (or all non-load `let` + `system` design variables) subject to every `.fsm` `assert` contract using `O(1)` mechanical & staggered thermo-elastic discrete adjoints (`trust_region_sqp_adjoint`). Updates the open session document when `apply` is true (default).","inputSchema":{"type":"object","properties":{"objective":{"type":"string","enum":["mass_g","compliance","max_vm_elem","max_displacement","max_temp"]},"params":{"type":"array","items":{"type":"string"}},"bounds":{"type":"object","additionalProperties":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2}},"max_iters":{"type":"integer"},"dofs":{"type":"integer"},"h":{"type":"number"},"apply":{"type":"boolean"}}}},
 {"name":"analyze_sdf_channel","description":"Extract 2D flow cross-sections (`A(s)`, wetted perimeter `P_w(s)`, hydraulic diameter `D_h(s)`) directly from the open `.fsm` SDF along `axis` (`x|y|z`), march 1D Darcy-Weisbach + Gnielinski convective heat transfer, and optionally bind `Convection` / `Pressure` BCs onto `bind_region`.","inputSchema":{"type":"object","properties":{"axis":{"type":"string","enum":["x","y","z"]},"passage_mode":{"type":"string","enum":["auto","interior_void","solid_core"]},"node_id":{"type":"string"},"stations":{"type":"integer"},"span_mm":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},"slice_h_mm":{"type":"number"},"mass_flow_kg_s":{"type":"number"},"density_kg_m3":{"type":"number"},"viscosity_pa_s":{"type":"number"},"conductivity_w_m_k":{"type":"number"},"cp_j_kg_k":{"type":"number"},"roughness_mm":{"type":"number"},"inlet_pressure_mpa":{"type":"number"},"inlet_temp_c":{"type":"number"},"wall_temp_c":{"type":"number"},"bind_region":{"type":"string"},"thermal_case":{"type":"string"},"pressure_case":{"type":"string"}}}},
 {"name":"export","description":"Mesh or slice-stack the session document (facts, and artifact bytes base64 when stl/mf are requested; pass `slices: true` for 2D `<s:slicestack>` export).","inputSchema":{"type":"object","properties":{"h":{"type":"number"},"layer_h":{"type":"number"},"slices":{"type":"boolean"},"adaptive":{"type":"boolean"},"solid_only":{"type":"boolean"},"stl":{"type":"boolean"},"mf":{"type":"boolean"}}}},
 {"name":"list_coupled_systems","description":"List all available CoupledEngineeringSystem models, including any user-authored declarative `system <id> { ... }` block in the open `.fsm` document plus built-in reference packs.","inputSchema":{"type":"object","additionalProperties":false}},
 {"name":"solve_coupled_system","description":"Solve a CoupledEngineeringSystem (defaults to the open `.fsm` document's declarative `system` block when present) at `design_p` and `epistemic_theta` via monolithic Newton + Dual-AD + Sparse Adjoint `dΦ/dp` + FOSM `p10/p50/p90` UQ, and project bound 0D/1D profiles onto the 3D `.fsm` session document (`sync_cad: true`).","inputSchema":{"type":"object","properties":{"system_id":{"type":"string"},"design_p":{"type":"array","items":{"type":"number"}},"epistemic_theta":{"type":"array","items":{"type":"number"}},"sync_cad":{"type":"boolean"}}}},
 {"name":"optimise_coupled_system","description":"Take `steps` (default 1) Sparse Adjoint `-dΦ/dp` design optimisation steps on a CoupledEngineeringSystem (including user-authored declarative `.fsm` `system` blocks) and project updated parameters and `lerp(...)` BCs onto the 3D `.fsm` session document.","inputSchema":{"type":"object","properties":{"system_id":{"type":"string"},"design_p":{"type":"array","items":{"type":"number"}},"epistemic_theta":{"type":"array","items":{"type":"number"}},"steps":{"type":"integer"},"sync_cad":{"type":"boolean"}}}},
 {"name":"solve_drone_antenna","description":"Solve the 4-inch DeadCat CFRP Drone + Gemini Dual-Band (`900 MHz` + `2.4 GHz` ELRS) Antenna VNA frequency sweep (`S11`, `S21`, `VSWR`, 50 Ω Smith Chart `Gamma(f)`, radiation efficiency `eta_rad`, CFRP eddy-current loss) and 3D radiation lobes (`G(theta, phi)`), and sync the 3D `.fsm` drone assembly.","inputSchema":{"type":"object","properties":{"preset":{"type":"string","enum":["optimised_default","unoptimised_tucked"]},"topology":{"type":"string","enum":["rear_tpu_v_dipole_pod","conformal_arm_folded_fpc","split_front_rear_orthogonal"]},"cfrp_standoff_mm":{"type":"number"},"low_band_arm_length_mm":{"type":"number"},"high_band_arm_length_mm":{"type":"number"},"diversity_skew_deg":{"type":"number"},"elevation_above_plate_mm":{"type":"number"},"dielectric_pod_wall_mm":{"type":"number"},"pod_eps_r":{"type":"number"},"pod_loss_tan":{"type":"number"},"cfrp_conductivity_s_per_m":{"type":"number"},"match_series_nh":{"type":"number"},"match_shunt_pf":{"type":"number"},"sync_cad":{"type":"boolean"}}}},
 {"name":"optimise_drone_antenna","description":"Optimise the 4-inch DeadCat CFRP Drone + Gemini Dual-Band (`900 MHz` + `2.4 GHz`) Antenna geometry (`cfrp_standoff_mm`, `elevation_above_plate_mm`, `low_band_arm_length_mm`, `high_band_arm_length_mm`, `diversity_skew_deg`, `dielectric_pod_wall_mm`) and L-network impedance match (`match_series_nh`, `match_shunt_pf`), returning initial vs. optimised VNA/Smith/Lobe reports and syncing the 3D `.fsm` drone assembly.","inputSchema":{"type":"object","properties":{"preset":{"type":"string","enum":["unoptimised_tucked","optimised_default"]},"topology":{"type":"string","enum":["rear_tpu_v_dipole_pod","conformal_arm_folded_fpc","split_front_rear_orthogonal"]},"cfrp_standoff_mm":{"type":"number"},"low_band_arm_length_mm":{"type":"number"},"high_band_arm_length_mm":{"type":"number"},"diversity_skew_deg":{"type":"number"},"elevation_above_plate_mm":{"type":"number"},"dielectric_pod_wall_mm":{"type":"number"},"iterations":{"type":"integer"},"sync_cad":{"type":"boolean"}}}},
 {"name":"discover_compute_peers","description":"Discover Fieldsmith compute servers (`fieldsmith-mcp --serve`) on the local subnet, loopback, or Tailscale/WireGuard interfaces via UDP broadcast (`:18790`) and parallel `/api/health` probes. Returns discovered peers sorted by highest thread count and lowest latency.","inputSchema":{"type":"object","properties":{"ports":{"type":"array","items":{"type":"integer"}},"timeout_ms":{"type":"integer"},"extra_hosts":{"type":"array","items":{"type":"string"}},"include_subnet_sweep":{"type":"boolean"}}}},
 {"name":"connect_compute_peer","description":"Connect this Fieldsmith instance to an external Fieldsmith compute server (e.g. a workstation running `fieldsmith-mcp --serve 0.0.0.0:8080`) by URL or `\"auto\"` (highest thread count peer), syncing the current `.fsm` workspace and offloading `solve` calls.","inputSchema":{"type":"object","properties":{"url":{"type":"string"},"extra_hosts":{"type":"array","items":{"type":"string"}}}}},
 {"name":"disconnect_compute_peer","description":"Disconnect from any attached external Fieldsmith compute server and return to local execution.","inputSchema":{"type":"object","additionalProperties":false}}
]"#;
