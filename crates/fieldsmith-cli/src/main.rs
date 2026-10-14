//! `fieldsmith` — run any Fieldsmith stage headless.
//!
//! ```text
//! fieldsmith check  FILE.fsm            parse + validate, print a summary
//! fieldsmith print  FILE.fsm            canonical .fsm on stdout
//! fieldsmith json   FILE.fsm            document as JSON
//! fieldsmith volume FILE.fsm [--h MM]   field volume with error estimate
//! fieldsmith glsl   FILE.fsm            generated preview shader
//! fieldsmith solve  FILE.fsm            mesh and solve the current setup
//! fieldsmith export FILE.fsm --stl X [--3mf Y] [--h MM] [--solid] [--adaptive]
//! ```

use fieldsmith_api::question;
use fieldsmith_api::question::{clock, parse_budget, DEFAULT_RUNGS};
use fieldsmith_api::store;
use fieldsmith_core::import::{import_mesh, ImportOpts};
use fieldsmith_core::rung::{CostFit, RungRecord, Tolerance};
use fieldsmith_core::{
    analyse, analyse_modes, fill::graded, fsmz, CaseResult, ModalRun, Resolution,
};
use fieldsmith_doc::{fsm, Document, NodeKind, Op};
use fieldsmith_field::meshfield::{key_from_hex, lookup};
use fieldsmith_field::{expr, glsl, volume};
use fieldsmith_mesh::export::{surface, write_3mf_volumes, TriMesh, Volume3mf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

use store::{Row, CSV_HEADER};

const USAGE: &str =
    "usage: fieldsmith <import|check|print|json|volume|glsl|solve|export> FILE [options]
       fieldsmith costs
       fieldsmith api

  check   parse and validate a document
  print   print the canonical .fsm form
  json    print the document as JSON
  volume  integrate the field volume   [--h <spacing mm>]
  glsl    print the generated preview shader
  costs   print the record row(s) of the last rung run as the data/costs/
          CSV — a redaction preview: exactly what a contribution PR would
          add, writes nothing, shape-only fields only
  api     read one JSON request on stdin, write one JSON envelope on
          stdout — ok and refusal both exit 0 (a refusal is an answer);
          a processing error is status:error, exit 2. The questions are
          the solver's same grammar: ladder (rungs/tolerance/budget),
          solve, modes. Reads the costs store for seeding; writes
          nothing.
  solve   mesh and solve               [--h <mm> | --dofs <n>] [--report]
          [--modes <n>] first n natural frequencies instead of the
          static cases: loads are ignored (supports still apply) and
          the solve is modal-only; fill documents are excluded
          (documents with a fill statement also get the graded re-solve)
          [--tolerance Q:U] declared-tolerance mode: refine down the rung
          schedule until the rung-to-rung change sits inside the box;
          Q is deflection|vm, U is mm|mpa|% (% is against the current
          rung's value) — e.g. deflection:0.1mm, deflection:1%, vm:2.5mpa.
          [--budget TIME] declared-budget mode: rungs run while measured
          elapsed plus the projected next rung (fit over the rungs so
          far, residual stated) stays inside TIME, e.g. 30s, 2m.
          [--rungs N,N,...] override the schedule (default 30000,150000,
          750000 DOF). Both modes answer with per-rung records; a box
          the rungs cannot meet is a named refusal and exit code 3
          (ordinary failures exit 1). Rungs record to the local costs
          store (~/.local/state/fieldsmith/costs.jsonl by default,
          FIELDSMITH_COSTS_STORE to move it); [--no-store] skips writing.
          The ladder runs the solid path: fill documents and --modes are
          refused alongside these flags.
  export  write a printable mesh       [--stl PATH] [--3mf PATH]
          [--h <mesh spacing mm>] (default 0.25); [--solid] exports the
          body without the fill; fill documents build the graded part
          and report its volume against the field volume.
          [--adaptive] meshes with adaptive dual contouring (finer only
          near the surface; needed for the 0.1 mm budget, carries the
          defect classes measured in V8)
  import  mesh an external part         [--out OUT.fsm] [--units m|mm|cm|in]
                                        [--up y|z] [--voxel MM] [--step-chord MM]
          SRC is a .glb, .stl, .obj, .3mf, .step or .stp file. STEP B-reps are
          tessellated from their analytic faces (planes, cylinders, spheres,
          cones, tori, B-splines) at the chord tolerance (--step-chord, mm
          sagitta, default derived from the model's size). Writes OUT.fsm
          (default: the
          source stem in the working directory) plus the .fsmz sidecar
          that stores its grids; volume/solve/export of the written
          document need that sidecar beside the .fsm.";

/// The rung schedule the declared-question modes walk by default lives in the API
/// crate (`fieldsmith_api::question::DEFAULT_RUNGS`, imported above): one schedule,
/// both presentation verbs.

/// Set when a declared question ends in a refusal: run() returns Ok (the refusal is an
/// honest answer, printed as such) and main exits 3 instead of 0 (GOAL v0.4 section A).
static REFUSED: AtomicBool = AtomicBool::new(false);
const REFUSAL_EXIT: u8 = 3;

/// Set when the `api` verb's envelope is a processing error: the body is still printed
/// (the envelope answers) and main exits 2, so a script that parses stdout is not left
/// exit-0 on a bad request (GOAL section C).
static API_ERR: AtomicBool = AtomicBool::new(false);

/// Load a document for the headless verbs. A `.fsmz` is read whole (grids included); a
/// plain `.fsm` hydrates its mesh grids from a same-stem `.fsmz` sibling when one sits
/// next to it — the text format carries no grids. A mesh document without its sidecar is
/// fine to inspect (`check`/`print`/`json`); the geometry verbs refuse it.
fn load(path: &str) -> Result<Document, String> {
    if path.ends_with(".fsmz") {
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        return fsmz::load(&bytes)
            .map(|l| l.doc)
            .map_err(|e| format!("{path}: {e}"));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let doc = fsm::parse(&text).map_err(|e| format!("{path}:{}: {}", e.line, e.message))?;
    if doc
        .root
        .iter()
        .any(|n| matches!(n.kind, NodeKind::Mesh { .. }))
    {
        let sidecar = std::path::Path::new(path).with_extension("fsmz");
        match std::fs::read(&sidecar) {
            Ok(bytes) => {
                fsmz::load(&bytes).map_err(|e| format!("{}: {e}", sidecar.display()))?;
            }
            Err(_) => eprintln!(
                "warning: {} holds mesh nodes but its .fsmz sidecar is missing; \
                 geometry verbs will refuse it",
                path
            ),
        }
    }
    Ok(doc)
}

/// Hashes of the document's mesh nodes whose registry grids are not present (an extend's
/// swept grid hydrates at compile time, so only the parent meshes can be checked here).
fn missing_grids(doc: &Document) -> Vec<String> {
    doc.root
        .iter()
        .filter_map(|n| match &n.kind {
            NodeKind::Mesh { hash, .. } => Some(hash.clone()),
            _ => None,
        })
        .filter(|h| lookup(key_from_hex(h)).is_none())
        .collect()
}

fn opt(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn run(args: &[String]) -> Result<(), String> {
    // costs takes no document — it reads the shape-only store and prints what a
    // contribution PR would add. It loads nothing, solves nothing, writes nothing.
    if args.first().map(|c| c.as_str()) == Some("costs") {
        return costs_cmd(args);
    }
    // api likewise takes no document path — the request itself carries the document.
    if args.first().map(|c| c.as_str()) == Some("api") {
        return api_cmd(args);
    }
    let (cmd, path) = match (args.first(), args.get(1)) {
        (Some(c), Some(p)) => (c.as_str(), p.as_str()),
        _ => return Err(USAGE.into()),
    };
    if cmd == "import" {
        return import_cmd(path, args);
    }
    let doc = load(path)?;
    // Only the inspection verbs work without the grid registry: everything that evaluates
    // the body would silently see an all-outside field.
    match cmd {
        "check" | "print" | "json" => {}
        _ => {
            let missing = missing_grids(&doc);
            if !missing.is_empty() {
                return Err(format!(
                    "mesh grid{} {} {} not loaded — the .fsmz sidecar must sit beside the \
                     .fsm (import the source again, or ship the sidecar)",
                    if missing.len() == 1 { "" } else { "s" },
                    missing.join(", "),
                    if missing.len() == 1 { "is" } else { "are" }
                ));
            }
        }
    }
    match cmd {
        "check" => {
            let problems = doc.validate();
            let thermal_cases: std::collections::BTreeSet<&str> =
                doc.temperatures.iter().map(|t| t.case.as_str()).collect();
            println!(
                "{}: {} shapes, {} regions, {} supports, {} loads, {} temperatures in {} case(s), material {}",
                doc.name,
                doc.node_ids().len(),
                doc.regions.len(),
                doc.supports.len(),
                doc.loads.len(),
                doc.temperatures.len(),
                thermal_cases.len(),
                doc.material
            );
            for n in &doc.root {
                if let NodeKind::Mesh { open_edges, .. } = &n.kind {
                    if *open_edges > 0 {
                        eprintln!(
                            "warning: mesh {} has {open_edges} open edge{} — the voxel \
                             sign flood is less reliable",
                            n.id,
                            if *open_edges == 1 { "" } else { "s" }
                        );
                    }
                }
            }
            if problems.is_empty() {
                println!("ok");
                for w in doc.material_warnings() {
                    println!("warning: {w}");
                }
                Ok(())
            } else {
                Err(problems.join("\n"))
            }
        }
        "print" => {
            print!("{}", fsm::print(&doc));
            Ok(())
        }
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "volume" => {
            let e = expr::compile_body(&doc);
            let b = expr::bounds(&e);
            if b.is_empty() {
                return Err("document has no solid".into());
            }
            let v = match opt(args, "--h") {
                Some(s) => volume::volume(
                    &e,
                    s.parse::<f64>()
                        .map_err(|_| format!("bad --h value: {s}"))?,
                ),
                None => volume::volume_auto(&e),
            };
            println!(
                "volume {:.3} mm³ ± {:.3} (grid {:.4} mm)",
                v.volume, v.error, v.spacing
            );
            Ok(())
        }
        "glsl" => {
            print!("{}", glsl::generate(&expr::compile_body(&doc)).source);
            Ok(())
        }
        "export" => export_cmd(&doc, args),
        "solve" => {
            // The declared-question modes (GOAL v0.4 section A): any ladder flag switches
            // the verb from "solve once" to "walk the schedule and answer".
            if opt(args, "--rungs").is_some()
                || opt(args, "--tolerance").is_some()
                || opt(args, "--budget").is_some()
            {
                return solve_ladder(&doc, args);
            }
            let res = if let Some(s) = opt(args, "--dofs") {
                Resolution::Dofs(
                    s.parse::<usize>()
                        .map_err(|_| format!("bad --dofs value: {s}"))?,
                )
            } else if let Some(s) = opt(args, "--h") {
                Resolution::Size(
                    s.parse::<f64>()
                        .map_err(|_| format!("bad --h value: {s}"))?,
                )
            } else {
                Resolution::Dofs(150_000)
            };
            let report = args.iter().any(|a| a == "--report");
            for w in doc.material_warnings() {
                eprintln!("warning: {w}");
            }
            if let Some(s) = opt(args, "--modes") {
                let count = s
                    .parse::<usize>()
                    .map_err(|_| format!("bad --modes value: {s}"))?;
                if doc.fill.is_some() {
                    return Err(
                        "modal runs the solid path — remove the fill statement to use --modes"
                            .into(),
                    );
                }
                return modal_cmd(&doc, &res, count, report);
            }
            if doc.fill.is_some() {
                return solve_graded(&doc, &res, report);
            }
            let a = analyse(&doc, res, clock).map_err(|e| e.0)?;
            let p = &a.provenance;
            if report {
                println!("## solve report — {}\n", doc.name);
                println!(
                    "P2 tetrahedra, element size {:.2} mm, {:.0} elements, {} DOF. Mesh {:.0} ms, \
                     solve {:.0} ms, {} iterations{}. Peaks are element-centroid von Mises.",
                    p.element_size,
                    p.elements,
                    p.dofs,
                    p.mesh_ms,
                    p.solve_ms,
                    p.iterations,
                    if p.thermal_ms > 0.0 {
                        format!(", thermal {:.0} ms", p.thermal_ms)
                    } else {
                        String::new()
                    }
                );
                println!(
                    "| case | peak von Mises (MPa) | max deflection (mm) |\n\
                     | :-- | --: | --: |"
                );
                for c in &a.cases {
                    println!(
                        "| {} | {:.2} | {:.4} |",
                        c.name, c.max_vm_elem, c.max_displacement
                    );
                }
                for c in &a.cases {
                    if let Some(r) = &c.conduction {
                        // A thermal case always carries a nodal temperature field (the
                        // conduction solution); guard anyway so a degenerate mesh, were
                        // one to reach here, cannot print inf.
                        match c.temperature.as_deref() {
                            Some(t) if !t.is_empty() => {
                                let lo = t.iter().cloned().fold(f64::INFINITY, f64::min);
                                let hi = t.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                                println!(
                                    "thermal case '{}': conduction {} iterations, nodal range {:.1}–{:.1} °C",
                                    c.name, r.iterations, lo, hi
                                );
                            }
                            _ => {
                                println!(
                                    "thermal case '{}': conduction {} iterations",
                                    c.name, r.iterations
                                );
                            }
                        }
                    }
                }
                if let Some(m) = &a.modes {
                    println!("\n## modes (solve modes={})\n", m.frequencies.len());
                    println!("| mode | frequency (Hz) |\n| :-- | --: |");
                    for (mi, &f) in m.frequencies.iter().enumerate() {
                        println!("| {} | {:.2} |", mi + 1, f);
                    }
                }
                print_load_table(&a.cases);
            } else {
                println!(
                    "{}: {} elements (P2), {} DOF, element size {:.2} mm",
                    doc.name, p.elements, p.dofs, p.element_size
                );
                println!(
                    "mesh {:.0} ms, solve {:.0} ms, {} iterations, min dihedral {:.1}°{}",
                    p.mesh_ms,
                    p.solve_ms,
                    p.iterations,
                    p.quality.min_dihedral_deg,
                    if p.thermal_ms > 0.0 {
                        format!(", thermal {:.0} ms", p.thermal_ms)
                    } else {
                        String::new()
                    }
                );
                for c in &a.cases {
                    println!(
                        "{}: peak von Mises {:.2} MPa (element centroids; nodal {:.2}), max deflection {:.4} mm{}",
                        c.name,
                        c.max_vm_elem,
                        c.max_von_mises,
                        c.max_displacement,
                        c.conduction
                            .as_ref()
                            .map(|r| format!(", conduction {} iterations", r.iterations))
                            .unwrap_or_default()
                    );
                }
                if let Some(m) = &a.modes {
                    for (mi, &f) in m.frequencies.iter().enumerate() {
                        println!("mode {}: {:.2} Hz", mi + 1, f);
                    }
                }
            }
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

/// `import`: read a GLB / STL / OBJ, voxelise it into the mesh registry, and write a fresh
/// document (one mesh node per scene part) plus its `.fsmz` sidecar. The document is the
/// bare import — regions, selectors and extends are authored in the `.fsm` by hand or in
/// the app.
fn import_cmd(src: &str, args: &[String]) -> Result<(), String> {
    let path = std::path::Path::new(src);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !matches!(ext.as_str(), "glb" | "stl" | "obj" | "3mf" | "step" | "stp") {
        return Err(format!(
            "import: source must be .glb, .stl, .obj, .3mf, .step or .stp, got .{ext}"
        ));
    }
    // .stp is an alias; the core kind is "step"
    let kind = if ext == "stp" { "step" } else { ext.as_str() };
    let units = opt(args, "--units");
    let up = opt(args, "--up");
    let voxel = match opt(args, "--voxel") {
        Some(s) => Some(
            s.parse::<f64>()
                .map_err(|_| format!("bad --voxel value: {s}"))?,
        ),
        None => None,
    };
    let step_chord = match opt(args, "--step-chord") {
        Some(s) => Some(
            s.parse::<f64>()
                .map_err(|_| format!("bad --step-chord value: {s}"))?,
        ),
        None => None,
    };
    if let Some(s) = &units {
        if !matches!(s.as_str(), "mm" | "cm" | "m" | "in") {
            return Err(format!("bad --units value: {s} (mm | cm | m | in)"));
        }
    }
    if let Some(s) = &up {
        if !matches!(s.as_str(), "y" | "z") {
            return Err(format!("bad --up value: {s} (y | z)"));
        }
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("import: {src} has no file name"))?
        .to_string();
    let file_name = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(src)
        .to_string();

    let bytes = std::fs::read(src).map_err(|e| format!("{src}: {e}"))?;
    let opts = ImportOpts {
        units: units.as_deref(),
        up: up.as_deref(),
        voxel,
        src: &file_name,
        taken: &[],
        step_chord,
    };
    let out = import_mesh(kind, &bytes, &opts)?;

    let doc = Document {
        name: stem,
        root: out.nodes,
        ..Default::default()
    };

    let out_path = opt(args, "--out").unwrap_or_else(|| format!("{name}.fsm", name = doc.name));
    let zip_path = std::path::Path::new(&out_path).with_extension("fsmz");
    std::fs::write(&out_path, fsm::print(&doc)).map_err(|e| format!("{out_path}: {e}"))?;
    let zip = fsmz::save(&doc)?;
    std::fs::write(&zip_path, &zip).map_err(|e| format!("{}: {e}", zip_path.display()))?;

    println!(
        "imported {src}: {} part(s) in {} ms",
        out.parts.len(),
        out.ms
    );
    for p in &out.parts {
        println!(
            "  {}: {} triangles, {} welded verts, {} patches, {} open edges",
            p.id, p.tris, p.verts, p.patches, p.open_edges
        );
        println!(
            "    bbox {} mm, voxel {} mm, grid {} cells, {} ms, hash {}",
            p.bbox.map(|v| v.to_string()).join(" "),
            p.voxel,
            p.cells,
            p.ms,
            p.hash
        );
        if let Some(s) = &p.step {
            match s.volume {
                Some(v) => println!("    STEP: {} b-rep faces, flux volume {v:.3}", s.faces),
                None => println!("    STEP: {} b-rep faces, flux volume unavailable", s.faces),
            }
        }
    }
    if let Some(s) = &out.step {
        println!(
            "    STEP file: unit {}, chord {:.6} mm, {} solid(s), parse {} ms, tessellate {} ms",
            s.length_unit, s.chord, s.solids, s.parse_ms, s.tess_ms
        );
        for r in &s.refusals {
            println!("    STEP refusal: {r}");
        }
    }
    println!(
        "wrote {out_path} and {} ({:.1} MiB of grids) — geometry verbs need the sidecar beside the .fsm",
        zip_path.display(),
        zip.len() as f64 / (1 << 20) as f64
    );
    Ok(())
}

/// `export`: the part as a triangle mesh for printing. Fill documents export the graded
/// part (the fill is part of the design); `--solid` exports the body alone. The reported
/// volume comes from the same field the mesh was extracted from, so Δ is the mesher's
/// representation error, not a modelling difference.
fn export_cmd(doc: &Document, args: &[String]) -> Result<(), String> {
    let stl = opt(args, "--stl");
    let threemf = opt(args, "--3mf");
    if stl.is_none() && threemf.is_none() {
        return Err("export: give --stl PATH and/or --3mf PATH".into());
    }
    let h = match opt(args, "--h") {
        Some(s) => s
            .parse::<f64>()
            .map_err(|_| format!("bad --h value: {s}"))?,
        None => 0.25,
    };
    if !h.is_finite() || h <= 0.0 {
        return Err("bad --h value: must be positive and finite".into());
    }
    let solid_only = args.iter().any(|a| a == "--solid");
    // Adaptive dual contouring refines only near the surface, which is what makes the
    // 0.1 mm export budget reachable; see V8 for the measured defect classes against this
    // model (and the uniform mesher's, for comparison).
    let adaptive = args.iter().any(|a| a == "--adaptive");
    let t_all = clock();

    let e = expr::compile_body(doc);
    let b = expr::bounds(&e);
    if b.is_empty() {
        return Err("document has no solid".into());
    }

    let field: expr::Expr = if doc.fill.is_some() && !solid_only {
        let t0 = clock();
        let (_, g) = graded(doc, Resolution::Dofs(150_000), clock).map_err(|x| x.0)?;
        println!(
            "{}: graded fill (mass {:.2} g of {:.2} g solid) built in {:.1} s",
            doc.name,
            g.mass_g,
            g.solid_mass_g,
            (clock() - t0) / 1000.0
        );
        expr::Expr::Filled(Box::new(g.fill))
    } else {
        e
    };

    let t0 = clock();
    let field_ref = &field;
    let tri = if adaptive {
        let lo = [b.lo[0] - h, b.lo[1] - h, b.lo[2] - h];
        let hi = [b.hi[0] + h, b.hi[1] + h, b.hi[2] + h];
        fieldsmith_mesh::adaptive::surface_adaptive(
            |p| {
                let d = expr::eval(field_ref, p);
                fieldsmith_mesh::adaptive::Sample { v: d.v, g: d.g }
            },
            lo,
            hi,
            h,
            &fieldsmith_mesh::adaptive::AdaptiveOpts::default(),
        )
    } else {
        surface(
            |p: [f64; 3]| expr::eval(field_ref, p).v,
            [b.lo[0] - h, b.lo[1] - h, b.lo[2] - h],
            [b.hi[0] + h, b.hi[1] + h, b.hi[2] + h],
            h,
        )
    };
    let mesh_s = (clock() - t0) / 1000.0;

    let t0 = clock();
    // Reference pair at the mesh spacing (export.rs in the wasm crate explains why the
    // coarser auto spacing's Richardson band misses the truth on TPMS fill fields).
    let v = volume::volume(field_ref, h);
    let ref_s = (clock() - t0) / 1000.0;
    let v_mesh = tri.volume();
    let delta = if v.volume > 0.0 {
        (v_mesh - v.volume) / v.volume * 100.0
    } else {
        f64::NAN
    };

    let t0 = clock();
    let d = tri.defects();
    let scan_s = (clock() - t0) / 1000.0;

    println!(
        "mesh {} verts, {} tris at h={h} mm ({}, mesh {:.2} s, defect scan {:.2} s)",
        tri.verts.len(),
        tri.tris.len(),
        if adaptive {
            "adaptive dc"
        } else {
            "uniform dc"
        },
        mesh_s,
        scan_s
    );
    println!(
        "field volume {:.1} ± {:.1} mm³, mesh volume {:.1} mm³, Δ {:+.3} % (field ref {:.1} s)",
        v.volume, v.error, v_mesh, delta, ref_s
    );
    if d.none() {
        println!(
            "defects: none (closed, 2-manifold, consistently oriented, self-intersection-free)"
        );
    } else {
        println!(
            "defects: {} open edges, {} non-manifold, {} orientation, {} degenerate, {} self-intersections",
            d.open_edges,
            d.non_manifold_edges,
            d.orientation_defects,
            d.degenerate_triangles,
            d.self_intersections
        );
    }
    let stl_bytes = stl.as_ref().map(|_| tri.write_stl());
    // When the document is a plain stack of add-shaped nodes — nothing subtracted or
    // intersected, no group shell/offset/mirror/pattern modifiers, no per-node blends, no
    // exported fill — each node's own shape already is a volume of the part, so the 3MF
    // carries one object per node, each labelled with its material (the node's label, else
    // the document material). Slicers union the shipped objects, so the printed solid is
    // the same body, and per-volume labels travel with it; where two labelled volumes
    // overlap, the slicer's rule (first hit) states that overlap's material.
    // A subtract/intersect node, a modifier, or a blend shapes the body *beyond* the nodes'
    // own shapes, so no set of per-node volumes reconstructs it: those exports stay the
    // single-union body labelled with the document material, as before.
    let per_volume = (doc.fill.is_none() || solid_only) && volumes_ok(&doc.root);
    let mf_bytes = threemf.as_ref().map(|_| {
        let mut meshes: Vec<(String, String, String, TriMesh)> = Vec::new();
        if per_volume {
            for n in &doc.root {
                if n.suppressed {
                    continue;
                }
                let fe = expr::compile_node(doc, n);
                let nb = expr::bounds(&fe);
                if nb.is_empty() {
                    continue;
                }
                let fref = &fe;
                let t1 = clock();
                let part = if adaptive {
                    fieldsmith_mesh::adaptive::surface_adaptive(
                        |p| {
                            let d = expr::eval(fref, p);
                            fieldsmith_mesh::adaptive::Sample { v: d.v, g: d.g }
                        },
                        [nb.lo[0] - h, nb.lo[1] - h, nb.lo[2] - h],
                        [nb.hi[0] + h, nb.hi[1] + h, nb.hi[2] + h],
                        h,
                        &fieldsmith_mesh::adaptive::AdaptiveOpts::default(),
                    )
                } else {
                    surface(
                        |p: [f64; 3]| expr::eval(fref, p).v,
                        [nb.lo[0] - h, nb.lo[1] - h, nb.lo[2] - h],
                        [nb.hi[0] + h, nb.hi[1] + h, nb.hi[2] + h],
                        h,
                    )
                };
                let mat = n.material.clone().unwrap_or_else(|| doc.material.clone());
                let vol = volume::volume(fref, h);
                let vs = (clock() - t1) / 1000.0;
                println!(
                    "  volume {}: {} verts, {} tris, material {mat}, field volume {:.1} ± {:.1} mm³ ({:.2} s)",
                    n.id,
                    part.verts.len(),
                    part.tris.len(),
                    vol.volume,
                    vol.error,
                    vs
                );
                // Root-level volumes have no ancestors, so their effective colour is their
                // own (a group passes its colour down through the per-volume carve).
                meshes.push((n.id.clone(), mat, n.color.clone().unwrap_or_default(), part));
            }
            if !meshes.is_empty() {
                let vols: Vec<Volume3mf> = meshes
                    .iter()
                    .map(|(id, mat, colour, m)| Volume3mf {
                        name: id,
                        material: mat,
                        color: colour,
                        mesh: m,
                    })
                    .collect();
                return write_3mf_volumes(&vols);
            }
        }
        tri.write_3mf(&doc.name, &doc.material)
    });
    for (path, bytes, what) in [
        stl.as_deref()
            .zip(stl_bytes)
            .map(|(p, b)| (p, b, "binary STL")),
        threemf.as_deref().zip(mf_bytes).map(|(p, b)| (p, b, "3MF")),
    ]
    .into_iter()
    .flatten()
    {
        let t0 = clock();
        std::fs::write(path, &bytes).map_err(|x| format!("{path}: {x}"))?;
        println!(
            "wrote {what} {path} ({:.1} MiB, {:.2} s)",
            bytes.len() as f64 / (1 << 20) as f64,
            (clock() - t0) / 1000.0
        );
    }
    println!(
        "export total {:.2} s (h {h:.3} mm)",
        (clock() - t_all) / 1000.0
    );
    Ok(())
}

/// True when every non-suppressed top-level node is additive and shapes nothing beyond its
/// own geometry — the condition under which one object per node reconstructs the same solid
/// as the union. Suppressed nodes are skipped by the exporter either way, so they don't
/// disqualify.
fn volumes_ok(nodes: &[fieldsmith_doc::Node]) -> bool {
    nodes.iter().all(|n| {
        n.suppressed
            || (n.op == Op::Add
                && n.blend == 0.0
                && match &n.kind {
                    NodeKind::Group { modifiers, .. } => modifiers.is_empty(),
                    _ => true,
                })
    })
}

/// Class counts and the achieved design-density range of the graded result (min-wall
/// clamps included; the per-element values behind `rho_elem` are material fractions now).
fn graded_shape(g: &fieldsmith_core::fill::Graded) -> ([usize; 3], (f64, f64)) {
    let counts = g.class.iter().fold([0usize; 3], |mut m, &k| {
        m[k as usize] += 1;
        m
    });
    (counts, (g.rho_design[0], g.rho_design[1]))
}

fn print_case_table(cases: &[CaseResult], caption: &str) {
    println!("| {caption} case | peak von Mises (MPa) | max deflection (mm) |");
    println!("| :-- | --: | --: |");
    for c in cases {
        println!(
            "| {} | {:.2} | {:.4} |",
            c.name, c.max_vm_elem, c.max_displacement
        );
    }
}

fn print_case_lines(cases: &[CaseResult], tag: &str) {
    for c in cases {
        println!(
            "{tag} {}: peak von Mises {:.2} MPa (element centroids; nodal {:.2}), max deflection {:.4} mm",
            c.name, c.max_vm_elem, c.max_von_mises, c.max_displacement
        );
    }
}

/// Per-load provenance for `solve --report`: what each labelled load applied, on how
/// many boundary facets, with its resultant and moment about the document origin.
/// Distributed (`fn=`) loads also report the unscaled density resultant, the factor the
/// `total=` rescale applied (1 when absent), and the face-frame fit numbers.
fn print_load_table(cases: &[CaseResult]) {
    if !cases.iter().any(|c| !c.load_reports.is_empty()) {
        return;
    }
    println!(
        "\n| load | region | facets | resultant (N) | moment about origin (N·mm) | applied as |\n\
         | :-- | :-- | --: | :-- | :-- | :-- |"
    );
    for c in cases {
        for r in &c.load_reports {
            let vec = |v: &[f64; 3]| format!("({:.2}, {:.2}, {:.2})", v[0], v[1], v[2]);
            let applied = match (r.fit_rms, r.fit_extent) {
                (Some(rms), Some(ext)) => format!(
                    "{:.2} N along the fn's direction, ×{:.3} to apply (fit RMS {:.3} mm over a {:.1} mm extent)",
                    r.unscaled, r.scale, rms, ext
                ),
                _ => format!("{:.4}× setpoint", r.scale),
            };
            println!(
                "| {} | {} | {} | {} | {} | {} |",
                r.id,
                r.region,
                r.facets,
                vec(&r.resultant),
                vec(&r.moment),
                applied
            );
        }
    }
}

/// `solve --modes <n>`: the first n natural frequencies (M8), modal-only — loads are
/// ignored (frequencies need the part held, not pushed; the supports still apply), and a
/// fill document is refused (the modal path meshes the solid reference at one modulus).
/// Two rungs, the requested resolution and one coarser, so the change column shows how
/// far each frequency has settled with refinement.
fn modal_cmd(doc: &Document, res: &Resolution, count: usize, report: bool) -> Result<(), String> {
    let coarse = match *res {
        Resolution::Size(h) => Resolution::Size(h * 2.0),
        Resolution::Dofs(n) => Resolution::Dofs((n / 2).max(1)),
    };
    if !doc.loads.is_empty() {
        println!("loads ignored: a modal run needs the part held, not pushed");
    }
    let mut runs: Vec<ModalRun> = Vec::new();
    for r in [coarse, *res] {
        runs.push(analyse_modes(doc, r, clock, count).map_err(|e| e.0)?);
    }
    for (i, run) in runs.iter().enumerate() {
        let p = &run.provenance;
        let label = if i == 0 {
            "coarse rung"
        } else {
            "requested rung"
        };
        if report {
            println!("## mode report — {} ({label})\n", doc.name);
            println!(
                "P2 tetrahedra, element size {:.2} mm, {:.0} elements, {} DOF. Mesh {:.0} ms, \
                 eigen solve {:.0} ms.",
                p.element_size, p.elements, p.dofs, p.mesh_ms, run.modal.solve_ms
            );
            println!("| mode | frequency (Hz) |");
            println!("| :-- | --: |");
        } else {
            println!(
                "{label}: element size {:.2} mm ({:.0} elements, {} DOF; mesh {:.0} ms, eigen {:.0} ms)",
                p.element_size, p.elements, p.dofs, p.mesh_ms, run.modal.solve_ms
            );
        }
        for (m, &f) in run.modal.frequencies.iter().enumerate() {
            let change = if i > 0 {
                let prev = &runs[i - 1].modal.frequencies;
                prev.get(m).map(|p0| 100.0 * (f - p0) / p0)
            } else {
                None
            };
            match (report, change) {
                (true, _) => println!("| {} | {:.2} |", m + 1, f),
                (false, Some(c)) => {
                    println!(
                        "mode {}: {:.2} Hz ({:+.1} % from the coarser mesh)",
                        m + 1,
                        f,
                        c
                    )
                }
                (false, None) => println!("mode {}: {:.2} Hz", m + 1, f),
            }
        }
    }
    Ok(())
}

/// `solve` on a document with a fill statement: the solid reference solve first, then the
/// graded re-solve with mass and class summary.
fn solve_graded(doc: &Document, res: &Resolution, report: bool) -> Result<(), String> {
    let (a, g) = graded(doc, *res, clock).map_err(|e| e.0)?;
    let p = &a.provenance;
    let fill = &g.spec;
    let (counts, rho) = graded_shape(&g);
    let n = (counts[0] + counts[1] + counts[2]) as f64;
    let lighter = if g.solid_mass_g > 0.0 {
        (1.0 - g.mass_g / g.solid_mass_g) * 100.0
    } else {
        0.0
    };
    if report {
        println!("## solve report — {} (solid reference)\n", doc.name);
        println!(
            "P2 tetrahedra, element size {:.2} mm, {:.0} elements, {} DOF. Mesh {:.0} ms, solve {:.0} ms, {} iterations. Peaks are element-centroid von Mises.",
            p.element_size, p.elements, p.dofs, p.mesh_ms, p.solve_ms, p.iterations
        );
        print_case_table(&a.cases, "solid");
        print_load_table(&a.cases);
        println!(
            "\n## fill — {:?} {:?}, cell {} mm, skin {} mm, ρ {:.2}..{:.2}, curve {}, min wall {} mm\n",
            fill.family, fill.mode, fill.cell, fill.skin, fill.rho_min, fill.rho_max, fill.curve, fill.min_wall
        );
        println!(
            "mass {:.2} g vs {:.2} g solid ({:.0}% lighter); lattice {:.0}% of elements, skin/solid {:.0}%, void {:.0}%.",
            g.mass_g,
            g.solid_mass_g,
            lighter,
            100.0 * counts[1] as f64 / n,
            100.0 * counts[0] as f64 / n,
            100.0 * counts[2] as f64 / n
        );
        if counts[1] == 0 {
            println!("no lattice elements in the graded part");
        } else {
            println!(
                "achieved design lattice ρ {:.2}..{:.2}; grade σ_ref {:.2} MPa",
                rho.0, rho.1, g.sigma_ref
            );
        }
        println!(
            "grade {:.0} ms, graded re-solve {:.0} ms",
            g.fill_ms, g.solve_ms
        );
        println!();
        print_case_table(&g.cases, "graded");
    } else {
        println!(
            "{}: {} elements (P2), {} DOF, element size {:.2} mm",
            doc.name, p.elements, p.dofs, p.element_size
        );
        println!(
            "mesh {:.0} ms, solve {:.0} ms, {} iterations, min dihedral {:.1}°",
            p.mesh_ms, p.solve_ms, p.iterations, p.quality.min_dihedral_deg
        );
        print_case_lines(&a.cases, "solid");
        println!(
            "fill {:?} {:?} cell {} mm: mass {:.2} g (solid {:.2} g), lattice {:.0}% of elements",
            fill.family,
            fill.mode,
            fill.cell,
            g.mass_g,
            g.solid_mass_g,
            100.0 * counts[1] as f64 / n
        );
        if counts[1] > 0 {
            println!(
                "achieved design lattice ρ {:.2}..{:.2}, σ_ref {:.2} MPa",
                rho.0, rho.1, g.sigma_ref
            );
        }
        print_case_lines(&g.cases, "graded");
        println!(
            "grade {:.0} ms, graded re-solve {:.0} ms",
            g.fill_ms, g.solve_ms
        );
    }
    Ok(())
}

/// `solve` with the declared-question flags (GOAL v0.4 section A): walk the rung
/// schedule, record each rung, and answer — stop inside the declared tolerance, spend
/// up to the declared budget, or refuse by name (exit 3) when the box cannot be met.
/// Every rung's wall time is one host measurement around mesh+solve; the record fields
/// are the engine's own numbers. Records append to the local costs store unless
/// --no-store; the store is also what seeds the estimator before the ladder has an
/// internal fit.
fn solve_ladder(doc: &Document, args: &[String]) -> Result<(), String> {
    if opt(args, "--dofs").is_some() || opt(args, "--h").is_some() {
        return Err(
            "the ladder chooses its own resolutions — drop --dofs/--h, or drop the ladder flags"
                .into(),
        );
    }
    if opt(args, "--modes").is_some() {
        return Err(
            "--modes and the ladder are separate questions — run them as separate solves".into(),
        );
    }
    if doc.fill.is_some() {
        return Err(
            "the ladder runs the solid path — remove the fill statement to use the declared-question modes"
                .into(),
        );
    }
    for w in doc.material_warnings() {
        eprintln!("warning: {w}");
    }
    let report = args.iter().any(|a| a == "--report");
    let no_store = args.iter().any(|a| a == "--no-store");

    let rungs: Vec<usize> = match opt(args, "--rungs") {
        None => DEFAULT_RUNGS.to_vec(),
        Some(s) => {
            let bad = || {
                format!(
                    "bad --rungs value: {s} — comma-separated increasing DOF targets, \
                     e.g. 30000,150000 (a ladder needs at least two rungs: the change \
                     estimate needs a baseline)"
                )
            };
            let v: Vec<usize> = s
                .split(',')
                .map(|t| t.trim().parse::<usize>())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| bad())?;
            if v.len() < 2 || !v.windows(2).all(|w| w[0] < w[1]) {
                return Err(bad());
            }
            v
        }
    };
    let tol_raw: Option<String> = opt(args, "--tolerance");
    let tolerance: Option<Tolerance> = match &tol_raw {
        Some(s) => Some(Tolerance::parse(s)?),
        None => None,
    };
    let budget_raw: Option<String> = opt(args, "--budget");
    let budget_ms: Option<f64> = match &budget_raw {
        Some(s) => Some(
            parse_budget(s)
                .map_err(|_| format!("bad --budget value: {s:?} — expect e.g. 30s or 2m"))?
                * 1000.0,
        ),
        None => None,
    };

    let mut decl = format!(
        "ladder: rungs {}",
        rungs
            .iter()
            .map(|r| r.to_string())
            .collect::<Vec<_>>()
            .join(" → ")
    );
    if let Some(t) = &tol_raw {
        decl += &format!(", tolerance {t}");
    }
    if let Some(b) = &budget_raw {
        decl += &format!(", budget {b}");
    }
    println!("{decl}");

    let machine = store::Machine::probe();
    let store_path = if no_store { None } else { store::store_path() };
    let store_rows = match &store_path {
        Some(p) => store::read_all(p),
        None => Vec::new(),
    };
    let leaves = expr::active_leaves(doc).len();

    // ONE verified loop (GOAL section C): the runner walks the schedule, streams the
    // rung records and notes through this sink — the CLI's prints ARE the sink's
    // implementation — and the answer lines below come from the same composition the
    // API envelope uses. The store rows here are the CLI's side effect; the runner
    // never writes the store.
    struct CliSink;
    impl question::Sink for CliSink {
        fn record(&mut self, rec: &RungRecord) {
            print_rung_streaming(rec);
        }
        fn note(&mut self, text: &str) {
            println!("{text}");
        }
    }
    let q = question::Question {
        rungs,
        tolerance: tolerance.map(|t| (t, tol_raw.clone().unwrap_or_default())),
        budget_ms,
        store_rows,
        machine: machine.clone(),
        clock,
    };
    let o = question::run(doc, &q, &mut CliSink).map_err(|e| e.0)?;
    let total_ms = o.total_ms;

    // Store the rung rows first: the refusal that may follow is still measured data,
    // and `costs` must see the run that just happened.
    if let Some(p) = &store_path {
        let rows: Vec<Row> = o
            .records
            .iter()
            .map(|r| Row {
                schema: store::SCHEMA,
                month: store::month_now(),
                platform: "cli".into(),
                engine: store::engine_identity(),
                cpu: machine.cpu.clone(),
                cores: machine.cores,
                ram_gb: machine.ram_gb,
                rung: r.rung,
                target_dofs: r.target_dofs,
                dofs: r.dofs,
                elements: r.elements,
                field_leaves: leaves,
                wall_ms: r.wall_ms,
                deflection_change_rel: r.change.map(|c| c.deflection_rel),
                vm_change_rel: r.change.map(|c| c.vm_rel),
            })
            .collect();
        if let Err(e) = store::append(p, &rows) {
            eprintln!("warning: {e}");
        }
    }

    if report {
        print_records_report(&o.records, total_ms);
    }

    // Cost fit over the rungs actually run, then projections and stored history for
    // the rungs the answer did not buy.
    if let Some(f) = o.fit {
        let r = match f.residual {
            Some(r) => format!("{:.0} %", 100.0 * r),
            None => "unmeasured (two rungs — the fit is exact by construction)".into(),
        };
        println!(
            "cost fit: wall ∝ DOF^{:.2} over {} rungs, fit residual {r}",
            f.exponent,
            o.records.len()
        );
    }
    for p in &o.projections {
        if let Some(fit_ms) = p.fit_ms {
            println!(
                "projection: rung at {target} DOF about {:.1} s (fit over {} rungs)",
                fit_ms / 1000.0,
                o.records.len(),
                target = p.target_dofs
            );
        }
        if let (Some(med), Some(n)) = (p.seed_ms, p.seed_n) {
            println!(
                "from your recorded runs ({} machine class, {target} DOF): median {:.1} s over {n} run(s)",
                machine.cpu,
                med / 1000.0,
                target = p.target_dofs
            );
        }
    }

    if let Some(m) = &o.met {
        println!("met: {}", question::met_text(m, total_ms));
    } else if let Some(u) = &o.unmet {
        // Tolerance declared and unmet: the named refusal (GOAL section A). The rung
        // records above already show the numbers; this is the answer to the question.
        REFUSED.store(true, Ordering::Relaxed);
        println!("refusal: {}", question::refusal_text(u));
    } else if let Some(r) = &o.stop_reason {
        println!("{r} — tolerance-free run, so nothing is unmet.");
    }
    Ok(())
}

/// One rung's streamed line: the schedule aim, what the mesh actually built, the wall
/// time, the tracked envelope, and the change estimate against the previous rung.
fn print_rung_streaming(r: &RungRecord) {
    println!(
        "rung {}: {} DOF aimed, {} achieved, {} elements, h {:.2} mm, {:.2} s",
        r.rung,
        r.target_dofs,
        r.dofs,
        r.elements,
        r.element_size_mm,
        r.wall_ms / 1000.0
    );
    println!(
        "  peak von Mises {:.2} MPa, max deflection {:.4} mm",
        r.tracked.vm_mpa, r.tracked.deflection_mm
    );
    if let Some(c) = &r.change {
        println!(
            "  change vs rung {}: deflection {:+.1} % ({:+.4} mm), von Mises {:+.1} % \
             ({:+.2} MPa) — estimates, not bounds",
            r.rung - 1,
            100.0 * c.deflection_rel,
            c.deflection_mm,
            100.0 * c.vm_rel,
            c.vm_mpa
        );
    }
}

/// The --report records table for a ladder run.
fn print_records_report(records: &[RungRecord], total_ms: f64) {
    if records.is_empty() {
        return;
    }
    println!("\n## rung records\n");
    println!(
        "| rung | DOF aimed | DOF | elements | h (mm) | wall (s) | Δ deflection | Δ von Mises |\n\
         | :-- | --: | --: | --: | --: | --: | :-- | :-- |"
    );
    for r in records {
        let (dd, dv) = match &r.change {
            Some(c) => (
                format!("{:+.1} %", 100.0 * c.deflection_rel),
                format!("{:+.1} %", 100.0 * c.vm_rel),
            ),
            None => ("—".into(), "—".into()),
        };
        println!(
            "| {} | {} | {} | {} | {:.2} | {:.2} | {} | {} |",
            r.rung,
            r.target_dofs,
            r.dofs,
            r.elements,
            r.element_size_mm,
            r.wall_ms / 1000.0,
            dd,
            dv
        );
    }
    println!(
        "\nchanges are rung-to-rung estimates, not bounds; total {:.1} s.",
        total_ms / 1000.0
    );
}

/// `api`: the one-shot headless verb (GOAL v0.4 section C, slice 1). Reads one JSON
/// request on stdin, writes one JSON envelope on stdout. `ok` and `refusal` both exit
/// 0 (a refusal is an answer); a `status: error` envelope still prints and then exits
/// 2 via API_ERR. The verb itself writes nothing and reads no files — the request
/// carries the document, and store seeding is the API crate's read-only look.
fn api_cmd(args: &[String]) -> Result<(), String> {
    if args.len() > 1 {
        return Err(
            "api takes no arguments — the request on stdin carries everything\n\n".to_string()
                + USAGE,
        );
    }
    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)
        .map_err(|e| format!("api: reading stdin: {e}"))?;
    let reply = fieldsmith_api::respond(&input);
    println!("{}", reply.body);
    if reply.is_error {
        API_ERR.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// `costs`: the redaction preview. Prints the last run's rows exactly as the
/// `data/costs/` CSV, plus the cost fit and a projection. Reads the store; writes
/// nothing, ever — contribution is the user opening a pull request, not this command.
fn costs_cmd(args: &[String]) -> Result<(), String> {
    if args.len() > 1 {
        return Err("costs takes no document\n\n".to_string() + USAGE);
    }
    let path = store::store_path()
        .ok_or("costs: no store location — set HOME or FIELDSMITH_COSTS_STORE")?;
    let rows = store::read_all(&path);
    if rows.is_empty() {
        println!("no local records — run `fieldsmith solve FILE --tolerance ...` or with `--rungs` and rungs record as they run");
        return Ok(());
    }
    let start = rows.iter().rposition(|r| r.rung == 0).unwrap_or(0);
    let run = &rows[start..];
    println!(
        "# last run: {} rung(s); the rows below are exactly what a contribution PR would add — write-access: none",
        run.len()
    );
    println!("{CSV_HEADER}");
    for r in run {
        println!("{}", r.csv());
    }
    let pts: Vec<(f64, f64)> = run.iter().map(|r| (r.dofs as f64, r.wall_ms)).collect();
    match CostFit::of(&pts) {
        Some(f) => {
            let res = match f.residual {
                Some(r) => format!("{:.0} %", 100.0 * r),
                None => "unmeasured (two rungs)".into(),
            };
            println!("cost fit: wall ∝ DOF^{:.2}, fit residual {res}", f.exponent);
            println!(
                "at a 5× refined rung ({} DOF): about {:.1} s",
                run.last().unwrap_or(&rows[start]).dofs * 5,
                f.project(5.0 * run.last().unwrap_or(&rows[start]).dofs as f64) / 1000.0
            );
        }
        None => println!("cost fit: the last run has too few rows for a projection"),
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => {
            // A refusal is a successfully-run answer (its named text is on stdout) with
            // its own exit code, so scripts can ask for the box and hear "not met".
            if REFUSED.load(Ordering::Relaxed) {
                ExitCode::from(REFUSAL_EXIT)
            } else if API_ERR.load(Ordering::Relaxed) {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
