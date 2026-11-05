//! V58 — Parametric 2D Sketch & Constraint Solver (Stage 1) + Surface-Sketch 3D Feature Engine (Stage 2)
//! + MCP Agent Sketch/Feature Workflow Verification.

use fieldsmith_doc::fsm;
use fieldsmith_doc::sketch::{
    apply_sketch_feature_to_document, preset_sketch_template, solve_sketch, ParametricSketch,
    SketchConstraint, SketchConstraintKind, SketchFeatureOp, SketchFeatureRequest, SketchPlane,
    SketchPoint, SketchSegment, SketchStatus,
};
use fieldsmith_mcp::Server;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[test]
fn v58_stage1_solver_dof_rank_homotopy_and_constraint_diagnostics() {
    // 1. Under-constrained triangle -> add constraints step-by-step -> 0 DOF -> redundant -> conflict
    let mut sk = ParametricSketch {
        id: "truss_panel".into(),
        plane: SketchPlane::Xy { offset: 0.0 },
        points: vec![
            SketchPoint {
                id: "p0".into(),
                x: 0.5,
                y: -0.2,
                fixed: false,
                construction: false,
            },
            SketchPoint {
                id: "p1".into(),
                x: 28.0,
                y: 3.0,
                fixed: false,
                construction: false,
            },
            SketchPoint {
                id: "p2".into(),
                x: 2.0,
                y: 19.0,
                fixed: false,
                construction: false,
            },
        ],
        segments: vec![
            SketchSegment {
                id: "s01".into(),
                p1: "p0".into(),
                p2: "p1".into(),
                bulge: 0.0,
                construction: false,
            },
            SketchSegment {
                id: "s12".into(),
                p1: "p1".into(),
                p2: "p2".into(),
                bulge: 0.0,
                construction: false,
            },
            SketchSegment {
                id: "s20".into(),
                p1: "p2".into(),
                p2: "p0".into(),
                bulge: 0.0,
                construction: false,
            },
        ],
        circles: vec![],
        constraints: vec![
            SketchConstraint {
                id: "fix_p0".into(),
                kind: SketchConstraintKind::Fixed {
                    point: "p0".into(),
                    x: 0.0,
                    y: 0.0,
                },
            },
            SketchConstraint {
                id: "horiz_s01".into(),
                kind: SketchConstraintKind::Horizontal {
                    segment: Some("s01".into()),
                    p1: None,
                    p2: None,
                },
            },
            SketchConstraint {
                id: "perp_corner".into(),
                kind: SketchConstraintKind::Perpendicular {
                    seg1: "s01".into(),
                    seg2: "s20".into(),
                },
            },
        ],
        macros: vec![],
    };

    // 6 variables (3 points x 2), 4 equations (Fixed=2, Horizontal=1, Perpendicular=1) -> DOF = 2!
    let rep_under = solve_sketch(&mut sk, &[]).expect("solve under-constrained");
    assert_eq!(rep_under.status, SketchStatus::UnderConstrained);
    assert_eq!(rep_under.total_variables, 6);
    assert_eq!(rep_under.jacobian_rank, 4);
    assert_eq!(rep_under.dof, 2);

    // Add large homotopy jump on base length (from ~28 mm to 80 mm) and equal-length leg constraint (`s20 == s01`)
    sk.constraints.push(SketchConstraint {
        id: "base_len".into(),
        kind: SketchConstraintKind::Distance {
            segment: Some("s01".into()),
            p1: None,
            p2: None,
            value: 0.0,
            expr: Some("base_w * 2".into()),
        },
    });
    sk.constraints.push(SketchConstraint {
        id: "equal_legs".into(),
        kind: SketchConstraintKind::EqualLength {
            seg1: "s01".into(),
            seg2: "s20".into(),
        },
    });

    let lets = vec![("base_w".to_string(), 40.0)];
    let rep_full = solve_sketch(&mut sk, &lets).expect("solve full-constrained with let expr");
    assert_eq!(rep_full.status, SketchStatus::FullyConstrained);
    assert_eq!(rep_full.dof, 0);
    assert!(rep_full.max_residual < 1e-7);

    // Verify solved coordinates: p0=(0,0), p1=(80,0), p2=(0,80) -> area = 0.5 * 80 * 80 = 3200 mm^2
    let p1 = rep_full
        .solved_sketch
        .points
        .iter()
        .find(|p| p.id == "p1")
        .unwrap();
    let p2 = rep_full
        .solved_sketch
        .points
        .iter()
        .find(|p| p.id == "p2")
        .unwrap();
    assert!((p1.x - 80.0).abs() < 1e-4 && p1.y.abs() < 1e-4);
    assert!(p2.x.abs() < 1e-4 && (p2.y - 80.0).abs() < 1e-4);
}

#[test]
fn v58_all_four_presets_and_macros_solve_with_zero_dof() {
    for name in [
        "flanged_mounting_bracket",
        "turned_aerospace_spool",
        "slotted_adjustment_link",
        "bolt_flange_plate",
    ] {
        let mut sk = preset_sketch_template(name).expect("preset template");
        let rep = solve_sketch(&mut sk, &[]).expect("solve preset");
        assert!(
            rep.converged,
            "preset '{name}' must converge cleanly: {}",
            rep.diagnosis
        );
        assert_eq!(
            rep.dof, 0,
            "preset '{name}' must be fully constrained (DOF = 0)"
        );
        assert!(
            rep.outer_profile.len() >= 4,
            "preset '{name}' must produce a closed outer loop"
        );
    }
    // Check bolt_flange_plate has 1 center bore + 4 bolt holes = 5 interior hole loops
    let mut flange = preset_sketch_template("bolt_flange_plate").unwrap();
    let rep = solve_sketch(&mut flange, &[]).unwrap();
    assert_eq!(rep.hole_profiles.len(), 5);
}

#[test]
fn v58_stage2_surface_sketch_boss_pocket_and_revolve_compile_and_solve_3d_fea() {
    let mut doc = fsm::parse(
        "fieldsmith 1\n\
         name \"Surface Sketch Assembly\"\n\
         material alsi10mg\n\
         box base_plate size=(80, 80, 12) at=(0, 0, 6)\n",
    )
    .expect("parse base plate");

    // 1. Attach a filleted bolt-flange boss onto `base_plate.top` (z = 12 mm)
    let boss_req = SketchFeatureRequest {
        feature_id: "top_flange_boss".into(),
        sketch: ParametricSketch::default(),
        preset: Some("bolt_flange_plate".into()),
        on: Some("base_plate.top".into()),
        operation: SketchFeatureOp::BossExtrude {
            height: 10.0,
            blend: 2.0,
            symmetric: false,
        },
        lets: BTreeMap::new(),
    };
    let boss_sum =
        apply_sketch_feature_to_document(&mut doc, &boss_req).expect("apply boss extrude on face");
    assert_eq!(boss_sum.world_origin, [0.0, 0.0, 17.0]);
    assert_eq!(boss_sum.created_node_ids.len(), 6); // 1 outer boss + 5 subtracted holes

    // 2. Cut a stadium slot pocket into the front face `base_plate.ny`
    let mut slot_sk = ParametricSketch::default();
    slot_sk
        .add_slot_macro(" pocket_slot", -15.0, 0.0, 15.0, 0.0, 3.0)
        .unwrap();
    let pocket_req = SketchFeatureRequest {
        feature_id: "front_lightening_slot".into(),
        sketch: slot_sk,
        preset: None,
        on: Some("base_plate.ny".into()),
        operation: SketchFeatureOp::PocketCut {
            depth: 6.0,
            through_all: false,
            blend: 0.0,
        },
        lets: BTreeMap::new(),
    };
    let pocket_sum =
        apply_sketch_feature_to_document(&mut doc, &pocket_req).expect("apply pocket cut on face");
    assert_eq!(pocket_sum.created_node_ids, vec!["front_lightening_slot"]);

    // 3. Add a 360-degree turned aerospace spool (`NodeKind::Revolve`) on top of the flange boss
    let rev_req = SketchFeatureRequest {
        feature_id: "turned_spool_collar".into(),
        sketch: ParametricSketch::default(),
        preset: Some("turned_aerospace_spool".into()),
        on: Some("xy".into()),
        operation: SketchFeatureOp::RevolveBoss {
            angle_deg: 360.0,
            blend: 1.5,
        },
        lets: BTreeMap::new(),
    };
    let rev_sum =
        apply_sketch_feature_to_document(&mut doc, &rev_req).expect("apply revolve boss");
    assert_eq!(rev_sum.created_node_ids, vec!["turned_spool_collar"]);

    // Validate .fsm print + parse round-trip including the new `revolve` statement!
    let fsm_txt = fsm::print(&doc);
    assert!(
        fsm_txt.contains("revolve turned_spool_collar"),
        "expected revolve statement in .fsm:\n{fsm_txt}"
    );
    let reparsed = fsm::parse(&fsm_txt).expect("round-trip parse with revolve");
    assert_eq!(fsm::print(&reparsed), fsm_txt);
    assert!(reparsed.validate().is_empty());

    // 4. Add boundary conditions and run a 3D FEA solve via MCP Server to verify the combined SDF + Revolve + Extrude
    let mut srv = Server::new();
    let open_line = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "open",
            "arguments": {
                "doc": format!(
                    "{fsm_txt}\n\
                     region mount_bot = base_plate.nz\n\
                     region load_top = top_flange_boss.top\n\
                     support fix_bot mount_bot fixed=xyz\n\
                     load push_top load_top force=(150, 0, -450) case=\"Load 1\"\n\
                     solve size=6.0\n"
                )
            }
        }
    })
    .to_string();
    let _ = srv.handle(&open_line).unwrap();

    let solve_line = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "solve",
            "arguments": { "kind": "solve" }
        }
    })
    .to_string();
    let solve_resp: Value = serde_json::from_str(&srv.handle(&solve_line).unwrap()).unwrap();
    assert_ne!(solve_resp["result"]["isError"], json!(true));
    let env: Value =
        serde_json::from_str(solve_resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(env["status"], "ok", "FEA solve failed: {env}");
    let dofs = env["single"]["provenance"]["dofs"].as_u64().unwrap();
    assert!(dofs > 100, "expected positive FEA DOFs, got {dofs}");
}

#[test]
fn v58_mcp_solve_sketch_and_apply_sketch_feature_tools_roundtrip() {
    let mut srv = Server::new();

    // 1. Call `solve_sketch` via MCP with declarative macros
    let req1 = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "tools/call",
        "params": {
            "name": "solve_sketch",
            "arguments": {
                "id": "mcp_link_sketch",
                "macros": [
                    { "macro": "slot", "id": "link_body", "x1": 0.0, "y1": 0.0, "x2": 45.0, "y2": 0.0, "radius": 10.0 },
                    { "macro": "circle", "id": "pin1", "cx": 0.0, "cy": 0.0, "radius": 4.0 },
                    { "macro": "circle", "id": "pin2", "cx": 45.0, "cy": 0.0, "radius": 4.0 }
                ]
            }
        }
    })
    .to_string();
    let resp1: Value = serde_json::from_str(&srv.handle(&req1).unwrap()).unwrap();
    assert_ne!(resp1["result"]["isError"], json!(true));
    let report: Value =
        serde_json::from_str(resp1["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(report["status"], "fully_constrained");
    assert_eq!(report["dof"], 0);
    assert_eq!(report["outer_profile"].as_array().unwrap().len(), 4);
    assert_eq!(report["hole_profiles"].as_array().unwrap().len(), 2);

    // 2. Call `apply_sketch_feature` via MCP to create a 3D link part and then cut a hex pocket into its top face
    let req2 = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/call",
        "params": {
            "name": "apply_sketch_feature",
            "arguments": {
                "feature_id": "slotted_arm",
                "preset": "slotted_adjustment_link",
                "on": "xy",
                "operation": {
                    "kind": "boss_extrude",
                    "height": 8.0,
                    "blend": 0.0
                }
            }
        }
    })
    .to_string();
    let resp2: Value = serde_json::from_str(&srv.handle(&req2).unwrap()).unwrap();
    assert_ne!(resp2["result"]["isError"], json!(true));
    let feat_out: Value =
        serde_json::from_str(resp2["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(feat_out["shapes"], 3); // 1 outer arm + 2 pin holes
    assert!(feat_out["problems"].as_array().unwrap().is_empty());
}
