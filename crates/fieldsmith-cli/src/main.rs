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

use fieldsmith_core::import::{import_mesh, ImportOpts};
use fieldsmith_core::{
    analyse, analyse_modes, fill::graded, fsmz, CaseResult, ModalRun, Resolution,
};
use fieldsmith_doc::{fsm, Document, NodeKind, Op};
use fieldsmith_field::meshfield::{key_from_hex, lookup};
use fieldsmith_field::{expr, glsl, volume};
use fieldsmith_mesh::export::{surface, TriMesh, Volume3mf, write_3mf_volumes};
use std::process::ExitCode;

const USAGE: &str =
    "usage: fieldsmith <import|check|print|json|volume|glsl|solve|export> FILE [options]

  check   parse and validate a document
  print   print the canonical .fsm form
  json    print the document as JSON
  volume  integrate the field volume   [--h <spacing mm>]
  glsl    print the generated preview shader
  solve   mesh and solve               [--h <mm> | --dofs <n>] [--report]
          [--modes <n>] first n natural frequencies instead of the
          static cases: loads are ignored (supports still apply) and
          the solve is modal-only; fill documents are excluded
          (documents with a fill statement also get the graded re-solve)
  export  write a printable mesh       [--stl PATH] [--3mf PATH]
          [--h <mesh spacing mm>] (default 0.25); [--solid] exports the
          body without the fill; fill documents build the graded part
          and report its volume against the field volume.
          [--adaptive] meshes with adaptive dual contouring (finer only
          near the surface; needed for the 0.1 mm budget, carries the
          defect classes measured in V8)
  import  mesh an external part         [--out OUT.fsm] [--units m|mm|cm|in]
                                        [--up y|z] [--voxel MM]
          SRC is a .glb, .stl, .obj or .3mf file. Writes OUT.fsm (default: the
          source stem in the working directory) plus the .fsmz sidecar
          that stores its grids; volume/solve/export of the written
          document need that sidecar beside the .fsm.";

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

fn clock() -> f64 {
    thread_local!(static T0: std::time::Instant = std::time::Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

fn run(args: &[String]) -> Result<(), String> {
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
            println!(
                "{}: {} shapes, {} regions, {} supports, {} loads, material {}",
                doc.name,
                doc.node_ids().len(),
                doc.regions.len(),
                doc.supports.len(),
                doc.loads.len(),
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
                     solve {:.0} ms, {} iterations. Peaks are element-centroid von Mises.",
                    p.element_size, p.elements, p.dofs, p.mesh_ms, p.solve_ms, p.iterations
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
                    "mesh {:.0} ms, solve {:.0} ms, {} iterations, min dihedral {:.1}°",
                    p.mesh_ms, p.solve_ms, p.iterations, p.quality.min_dihedral_deg
                );
                for c in &a.cases {
                    println!(
                        "{}: peak von Mises {:.2} MPa (element centroids; nodal {:.2}), max deflection {:.4} mm",
                        c.name, c.max_vm_elem, c.max_von_mises, c.max_displacement
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
    let kind = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !matches!(kind.as_str(), "glb" | "stl" | "obj" | "3mf") {
        return Err(format!(
            "import: source must be .glb, .stl, .obj or .3mf, got .{kind}"
        ));
    }
    let units = opt(args, "--units");
    let up = opt(args, "--up");
    let voxel = match opt(args, "--voxel") {
        Some(s) => Some(
            s.parse::<f64>()
                .map_err(|_| format!("bad --voxel value: {s}"))?,
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
    };
    let out = import_mesh(&kind, &bytes, &opts)?;

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
        let mut meshes: Vec<(String, String, TriMesh)> = Vec::new();
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
                meshes.push((n.id.clone(), mat, part));
            }
            if !meshes.is_empty() {
                let vols: Vec<Volume3mf> = meshes
                    .iter()
                    .map(|(id, mat, m)| Volume3mf {
                        name: id,
                        material: mat,
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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
