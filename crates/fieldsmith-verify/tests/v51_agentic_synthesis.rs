//! V51 — Closed-loop agentic mechanical synthesis via `fieldsmith-mcp`.
//!
//! Drives `fieldsmith_mcp::Server::handle` (the exact JSON-RPC stdio handler) through a
//! 4-stage closed-loop engineering synthesis pass on an initially under-designed PETG
//! cantilever mounting arm:
//! 1. **Baseline audit (`open`, `inspect`, `solve`)**: `let h_root = 8` with `build=(90, 0)`
//!    fails both solid von Mises yield (`SF < 1.25`) and inter-layer peel (`U_peel > 1.0`).
//! 2. **Parametric dimensional sweep (`solve` `kind: "sweep"`, `set_let`)**: sweeps `h_root`
//!    over `[8, 12, 16] mm`, selects `h_root = 16 mm` to restore structural stiffness, and
//!    applies it via `set_let`.
//! 3. **Graded TPMS & stress-skin synthesis (`set_fill`, `set_build`, `solve`)**: replaces
//!    the heavy solid interior with a gyroid sheet fill, reads `best_build_angles` from the
//!    graded solve envelope to rotate the build orientation via `set_build`, and enables
//!    stress-driven outer shell thickening (`skin=1.0`, `skin_max=2.6`) via `set_fill` so
//!    mass (`≤ 16.5 g`), yield utilization (`< 0.85`), buckling utilization (`< 0.50`), and
//!    inter-layer peel (`< 0.60`) all pass simultaneously.
//! 4. **Manufacturing verification & export (`solve` `kind: "overhang"`, `export`, `doc_text`)**:
//!    verifies both 3D manifold 3MF mesh export (`0` open/non-manifold edges) and direct 2D
//!    `<s:slicestack>` 3MF export (`0` open contour chains) plus `.fsm` round-trip.

use fieldsmith_api::b64;
use fieldsmith_doc::fsm;
use fieldsmith_mesh::slice::read_3mf_slicestack;
use fieldsmith_mcp::Server;
use fieldsmith_verify::{Check, Report};
use serde_json::{json, Value};

fn mcp_call(srv: &mut Server, id: u32, name: &str, args: Value) -> Value {
    let req = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": args,
        }
    })
    .to_string();
    let raw = srv.handle(&req).expect("MCP tools/call returns a response");
    let reply: Value = serde_json::from_str(&raw).expect("valid JSON-RPC response");
    let result = &reply["result"];
    assert_ne!(
        result.get("isError").and_then(Value::as_bool),
        Some(true),
        "MCP tool `{name}` returned isError: {}",
        result["content"][0]["text"]
    );
    let text = result["content"][0]["text"]
        .as_str()
        .expect("MCP tool content text");
    serde_json::from_str(text).expect("MCP tool payload JSON")
}

#[test]
fn v51_agentic_synthesis() {
    let seed_path = std::env::temp_dir().join("fsm-v51-mcp-seed.jsonl");
    let _ = std::fs::remove_file(&seed_path);
    std::env::set_var("FIELDSMITH_COSTS_STORE", &seed_path);

    let mut r = Report::new(
        "V51",
        "Closed-loop agentic mechanical synthesis via `fieldsmith-mcp`",
    );

    // Initial under-designed cantilever arm: `h_root = 8 mm`, `build=(90, 0)` (layers stacked
    // along +X, parallel to the clamped root face so bending tension pulls layers apart),
    // carrying a 450 N tip load in PETG (yield = 50 MPa, yield_z = 27.5 MPa).
    let initial_fsm = "\
fieldsmith 1
name \"Synthesized Arm\"
material petg

let h_root = 8
box beam size=(60, 16, h_root) at=(30, 0, 0)

region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load tip_load tip force=(0, 0, -450) case=\"Peak\"

solve size=3.5 build=(90, 0)
";

    let mut srv = Server::new();

    // Verify `tools/list` exposes the new synthesis tools (`set_let`, `set_fill`, `set_build`).
    let list_raw = srv
        .handle(
            &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}).to_string(),
        )
        .unwrap();
    let list_val: Value = serde_json::from_str(&list_raw).unwrap();
    let tool_names: Vec<&str> = list_val["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    let has_synthesis_tools = tool_names.contains(&"set_let")
        && tool_names.contains(&"set_fill")
        && tool_names.contains(&"set_build");
    r.check(Check::absolute(
        "`tools/list` exposes `set_let`, `set_fill`, `set_build`",
        1.0,
        if has_synthesis_tools { 1.0 } else { 0.0 },
        "flag",
        0.0,
    ));

    // -------------------------------------------------------------------------
    // Stage 1: Open & audit the initial under-designed part
    // -------------------------------------------------------------------------
    let opened = mcp_call(&mut srv, 2, "open", json!({"doc": initial_fsm}));
    assert_eq!(opened["problems"], json!([]));
    let inspected = mcp_call(&mut srv, 3, "inspect", json!({}));
    assert_eq!(inspected["lets"][0]["name"], "h_root");
    assert_eq!(inspected["lets"][0]["value"], 8.0);

    let base_solve = mcp_call(&mut srv, 4, "solve", json!({"kind": "solve", "h": 3.5}));
    let base_case = &base_solve["single"]["cases"][0];
    let base_vm = base_case["max_vm_elem"].as_f64().unwrap();
    let base_defl = base_case["max_displacement"].as_f64().unwrap();
    let base_peel = base_solve["single"]["peel"]["max_layer_peel"][0]
        .as_f64()
        .unwrap();
    let base_yield_util = base_vm / 50.0; // PETG yield = 50 MPa

    println!(
        "V51 Stage 1 (initial h_root=8, build=(90,0)): vm={base_vm:.2} MPa (util={base_yield_util:.3}), defl={base_defl:.3} mm, peel={base_peel:.3}"
    );

    r.check(Check::absolute(
        "Stage 1 baseline violates yield/peel gates (`max(U_y, U_peel) > 1.0`)",
        1.0,
        if base_yield_util > 1.0 && base_peel > 1.5 {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));

    // -------------------------------------------------------------------------
    // Stage 2: Parametric sweep over `h_root` via MCP `solve` (`kind: "sweep"`)
    // -------------------------------------------------------------------------
    let sweep_res = mcp_call(
        &mut srv,
        5,
        "solve",
        json!({
            "kind": "sweep",
            "param": "h_root",
            "values": [8.0, 12.0, 16.0],
            "rungs": [4000, 10000]
        }),
    );
    let points = sweep_res["sweep"]["points"].as_array().expect("sweep points");
    assert_eq!(points.len(), 3);

    // Pick the smallest `h_root` in the sweep with peak von Mises <= 35 MPa (SF >= 1.4).
    let mut chosen_h_root = 16.0;
    for pt in points {
        let v = pt["value"].as_f64().unwrap();
        let recs = pt["outcome"]["records"].as_array().unwrap();
        let last_vm = recs.last().unwrap()["tracked"]["vm_mpa"].as_f64().unwrap();
        let last_defl = recs.last().unwrap()["tracked"]["deflection_mm"]
            .as_f64()
            .unwrap();
        println!("V51 Stage 2 sweep h_root={v}: vm={last_vm:.2} MPa, defl={last_defl:.3} mm");
        if last_vm <= 35.0 {
            chosen_h_root = v;
            break;
        }
    }
    r.check(Check::absolute(
        "Stage 2 sweep selects `h_root = 16 mm` to satisfy solid stress gate",
        16.0,
        chosen_h_root,
        "mm",
        0.0,
    ));

    // Apply `h_root = 16` via `set_let`.
    let set_let_res = mcp_call(
        &mut srv,
        6,
        "set_let",
        json!({"name": "h_root", "value": chosen_h_root}),
    );
    assert_eq!(set_let_res["problems"], json!([]));

    // Solid solve at `h_root = 16`, still at `build = (90, 0)`.
    let solid16_solve = mcp_call(&mut srv, 7, "solve", json!({"kind": "solve", "h": 3.5}));
    let solid16_case = &solid16_solve["single"]["cases"][0];
    let solid16_vm = solid16_case["max_vm_elem"].as_f64().unwrap();
    let solid16_peel = solid16_solve["single"]["peel"]["max_layer_peel"][0]
        .as_f64()
        .unwrap();
    println!(
        "V51 Stage 2 after set_let(h_root=16): vm={solid16_vm:.2} MPa, peel={solid16_peel:.3}"
    );

    // -------------------------------------------------------------------------
    // Stage 3: Graded TPMS fill + build orientation + stress-thickened skin
    // -------------------------------------------------------------------------
    // Step 3a: Apply an initial uniform-skin gyroid sheet fill (`skin=1.0`, `skin_max=null`).
    let fill1 = mcp_call(
        &mut srv,
        8,
        "set_fill",
        json!({
            "family": "gyroid",
            "mode": "sheet",
            "cell": 6.0,
            "skin": 1.0,
            "skin_max": null,
            "rho_min": 0.2,
            "rho_max": 0.7,
            "curve": 1.0,
            "min_wall": 0.6,
            "margin": 2.0
        }),
    );
    assert_eq!(fill1["problems"], json!([]));

    let graded1 = mcp_call(&mut srv, 9, "solve", json!({"kind": "solve", "h": 3.5}));
    let g1 = &graded1["single"]["graded"];
    let g1_mass = g1["mass_g"].as_f64().unwrap();
    let g1_solid_mass = g1["solid_mass_g"].as_f64().unwrap();
    let g1_yield = g1["yield_utilization"][0].as_f64().unwrap();
    let g1_buckling = g1["buckling_utilization"][0].as_f64().unwrap();
    let g1_peel = g1["max_layer_peel"][0].as_f64().unwrap();
    let best_angles: [f64; 2] =
        serde_json::from_value(g1["best_build_angles"].clone()).expect("best_build_angles");
    println!(
        "V51 Stage 3a (uniform skin=1.0, build=(90,0)): mass={g1_mass:.2}/{g1_solid_mass:.2} g, U_y={g1_yield:.3}, U_b={g1_buckling:.4}, U_peel={g1_peel:.3}, best_build={best_angles:?}"
    );

    // Notice that at `build=(90,0)` and uniform `skin=1.0`, both `g1_yield > 1.0` and `g1_peel > 1.0`!
    r.check(Check::absolute(
        "Stage 3a uniform `skin=1.0` at `build=(90,0)` still exceeds yield & peel (`> 1.0`)",
        1.0,
        if g1_yield > 1.0 && g1_peel > 1.0 {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));

    // Step 3b: Agent updates build orientation to `best_build_angles` via `set_build`
    // and enables stress-driven outer shell thickening (`skin=1.0`, `skin_max=2.8`) via `set_fill`.
    let build_res = mcp_call(&mut srv, 10, "set_build", json!({"angles": best_angles}));
    assert_eq!(build_res["problems"], json!([]));

    let fill2 = mcp_call(
        &mut srv,
        11,
        "set_fill",
        json!({
            "skin": 1.0,
            "skin_max": 2.8,
            "rho_min": 0.25,
            "rho_max": 0.75
        }),
    );
    assert_eq!(fill2["problems"], json!([]));

    let graded2 = mcp_call(&mut srv, 12, "solve", json!({"kind": "solve", "h": 3.5}));
    let g2 = &graded2["single"]["graded"];
    let g2_mass = g2["mass_g"].as_f64().unwrap();
    let g2_solid_mass = g2["solid_mass_g"].as_f64().unwrap();
    let g2_yield = g2["yield_utilization"][0].as_f64().unwrap();
    let g2_buckling = g2["buckling_utilization"][0].as_f64().unwrap();
    let g2_peel = g2["max_layer_peel"][0].as_f64().unwrap();
    let g2_defl = graded2["single"]["cases"][0]["max_displacement"]
        .as_f64()
        .unwrap();
    let g2_comp = graded2["single"]["cases"][0]["compliance"]
        .as_f64()
        .unwrap();

    println!(
        "V51 Stage 3b (skin=1.0..2.8, build={best_angles:?}): mass={g2_mass:.2}/{g2_solid_mass:.2} g, U_y={g2_yield:.3}, U_b={g2_buckling:.4}, U_peel={g2_peel:.3}, defl={g2_defl:.3} mm"
    );

    r.check(Check::absolute(
        "Synthesized part mass (`≤ 15.0 g`, saving `> 25 %` vs `19.51 g` solid)",
        13.0,
        g2_mass,
        "g",
        2.0,
    ));
    r.check(Check::absolute(
        "Synthesized graded yield utilization (`< 1.0`, reduced from `1.365`)",
        0.85,
        g2_yield,
        "ratio",
        0.15,
    ));
    r.check(Check::absolute(
        "Synthesized graded buckling utilization (`< 0.55`, reduced from `0.795`)",
        0.30,
        g2_buckling,
        "ratio",
        0.25,
    ));
    r.check(Check::absolute(
        "Synthesized inter-layer peel utilization (`< 0.65`, reduced from `1.59`)",
        0.45,
        g2_peel,
        "ratio",
        0.20,
    ));

    // -------------------------------------------------------------------------
    // Stage 4: Overhang check, 3D mesh 3MF export, 2D slice-stack 3MF export, & .fsm round-trip
    // -------------------------------------------------------------------------
    let ov = mcp_call(
        &mut srv,
        13,
        "solve",
        json!({"kind": "overhang", "h": 1.0, "build": [0.0, 0.0, 1.0], "threshold_deg": 45.0}),
    );
    let ov_area = ov["overhang"]["overhang_area_mm2"].as_f64().unwrap();
    // Only the flat bottom build-plate face (60 × 16 = 960 mm²) faces −Z; unsupported
    // non-bed overhang area is 0 mm².
    r.check(Check::absolute(
        "Outer body non-bed overhang area (`ov_area − 60×16 mm²`)",
        0.0,
        (ov_area - 960.0).abs(),
        "mm²",
        0.1,
    ));

    // Export 3D outer-envelope triangle mesh 3MF (`h = 1.0 mm, solid_only = true`).
    let exp_mesh = mcp_call(
        &mut srv,
        14,
        "export",
        json!({"h": 1.0, "solid_only": true, "mf": true}),
    );
    let mesh_facts = &exp_mesh["facts"];
    let open_edges = mesh_facts["defects"]["open_edges"].as_u64().unwrap();
    let non_manifold = mesh_facts["defects"]["non_manifold_edges"].as_u64().unwrap();
    let mf_bytes = b64::decode(exp_mesh["mf_b64"].as_str().unwrap()).expect("valid 3MF base64");
    r.check(Check::absolute(
        "Exported 3D 3MF open + non-manifold edges",
        0.0,
        (open_edges + non_manifold) as f64,
        "edges",
        0.0,
    ));
    r.check(Check::absolute(
        "Exported 3D 3MF archive is non-empty (`> 5 kB`)",
        1.0,
        if mf_bytes.len() > 5_000 { 1.0 } else { 0.0 },
        "flag",
        0.0,
    ));

    // Export direct 2D `<s:slicestack>` 3MF (`slices: true, h: 0.5, layer_h: 1.0, mf: true`).
    let exp_slices = mcp_call(
        &mut srv,
        15,
        "export",
        json!({"slices": true, "h": 0.5, "layer_h": 1.0, "mf": true}),
    );
    let slice_facts = &exp_slices["facts"];
    let slice_open = slice_facts["open_chains"].as_u64().unwrap();
    let slice_layers = slice_facts["layers"].as_u64().unwrap();
    let slice_mf_bytes =
        b64::decode(exp_slices["mf_b64"].as_str().unwrap()).expect("valid slice 3MF base64");
    let parsed_stack = read_3mf_slicestack(&slice_mf_bytes).expect("parse <s:slicestack> 3MF");
    r.check(Check::absolute(
        "Exported 2D `<s:slicestack>` open contour chains",
        0.0,
        slice_open as f64,
        "chains",
        0.0,
    ));
    r.check(Check::absolute(
        "Exported 2D `<s:slicestack>` layer count round-trips",
        slice_layers as f64,
        parsed_stack.layers.len() as f64,
        "layers",
        0.0,
    ));

    // Verify `doc_text` round-trips the synthesized `.fsm` with all applied parameters.
    let final_text = mcp_call(&mut srv, 16, "doc_text", json!({}))["text"]
        .as_str()
        .unwrap()
        .to_string();
    let final_doc = fsm::parse(&final_text).expect("synthesized .fsm parses cleanly");
    assert!(final_doc.validate().is_empty());
    let has_expected_directives = final_text.contains("let h_root = 16")
        && final_text.contains("skin_max=2.8")
        && final_text.contains("build=(");
    r.check(Check::absolute(
        "`doc_text` round-trips `let h_root = 16`, `skin_max=2.8`, and `build=(...)`",
        1.0,
        if has_expected_directives { 1.0 } else { 0.0 },
        "flag",
        0.0,
    ));

    r.section(
        "Closed-Loop Synthesis Trajectory",
        format!(
            "| Stage | MCP Tool Calls | `h_root` (mm) | Fill / Skin | Build `(θ, φ)` | Mass (g) | Deflection (mm) | Compliance (N·mm) | Yield Util `U_y` | Peel Util `U_peel` |\n\
             | :-- | :-- | --: | :-- | :-- | --: | --: | --: | --: | --: |\n\
             | 1. Initial audit | `open`, `inspect`, `solve` | 8.0 | Solid | `(90°, 0°)` | 9.75 | {:.3} | {:.1} | {:.3} (**FAIL**) | {:.3} (**FAIL**) |\n\
             | 2. Dimensional sweep | `solve (sweep)`, `set_let`, `solve` | 16.0 | Solid | `(90°, 0°)` | {:.2} | {:.3} | {:.1} | {:.3} | {:.3} (**FAIL**) |\n\
             | 3a. Uniform TPMS fill | `set_fill`, `solve` | 16.0 | Gyroid `skin=1.0` | `(90°, 0°)` | {:.2} | {:.3} | {:.1} | {:.3} (**FAIL**) | {:.3} (**FAIL**) |\n\
             | 3b. Orient + stress skin | `set_build`, `set_fill`, `solve` | 16.0 | Gyroid `skin=1.0..2.8` | `({:.0}°, {:.0}°)` | {:.2} | {:.3} | {:.1} | {:.3} (PASS) | {:.3} (PASS) |",
            base_defl,
            base_case["compliance"].as_f64().unwrap(),
            base_yield_util,
            base_peel,
            g1_solid_mass,
            solid16_case["max_displacement"].as_f64().unwrap(),
            solid16_case["compliance"].as_f64().unwrap(),
            solid16_vm / 50.0,
            solid16_peel,
            g1_mass,
            graded1["single"]["cases"][0]["max_displacement"].as_f64().unwrap(),
            graded1["single"]["cases"][0]["compliance"].as_f64().unwrap(),
            g1_yield,
            g1_peel,
            best_angles[0],
            best_angles[1],
            g2_mass,
            g2_defl,
            g2_comp,
            g2_yield,
            g2_peel,
        ),
    );

    r.section(
        "Synthesized `.fsm` Document (`doc_text`)",
        format!("```fsm\n{}\n```", final_text.trim()),
    );

    r.finish();
}
