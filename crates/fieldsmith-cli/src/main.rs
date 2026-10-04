//! `fieldsmith` — run any Fieldsmith stage headless.
//!
//! ```text
//! fieldsmith check  FILE.fsm            parse + validate, print a summary
//! fieldsmith print  FILE.fsm            canonical .fsm on stdout
//! fieldsmith json   FILE.fsm            document as JSON
//! fieldsmith volume FILE.fsm [--h MM]   field volume with error estimate
//! fieldsmith glsl   FILE.fsm            generated preview shader
//! fieldsmith solve  FILE.fsm            mesh and solve the current setup
//! fieldsmith export FILE.fsm --stl X [--3mf Y] [--h MM] [--solid]
//! ```

use fieldsmith_core::fill::graded;
use fieldsmith_core::{analyse, CaseResult, Resolution};
use fieldsmith_doc::{fsm, Document};
use fieldsmith_field::{expr, glsl, volume};
use fieldsmith_mesh::export::surface;
use std::process::ExitCode;

const USAGE: &str = "usage: fieldsmith <check|print|json|volume|glsl|solve|export> FILE.fsm [options]

  check   parse and validate a document
  print   print the canonical .fsm form
  json    print the document as JSON
  volume  integrate the field volume   [--h <spacing mm>]
  glsl    print the generated preview shader
  solve   mesh and solve               [--h <mm> | --dofs <n>] [--report]
          (documents with a fill statement also get the graded re-solve)
  export  write a printable mesh       [--stl PATH] [--3mf PATH]
          [--h <mesh spacing mm>] (default 0.25); [--solid] exports the
          body without the fill; fill documents build the graded part
          and report its volume against the field volume";

fn load(path: &str) -> Result<Document, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    fsm::parse(&text).map_err(|e| format!("{path}:{}: {}", e.line, e.message))
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
    let doc = load(path)?;
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
            if problems.is_empty() {
                println!("ok");
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
                Resolution::Dofs(s.parse::<usize>().map_err(|_| format!("bad --dofs value: {s}"))?)
            } else if let Some(s) = opt(args, "--h") {
                Resolution::Size(s.parse::<f64>().map_err(|_| format!("bad --h value: {s}"))?)
            } else {
                Resolution::Dofs(150_000)
            };
            let report = args.iter().any(|a| a == "--report");
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
                    println!("| {} | {:.2} | {:.4} |", c.name, c.max_vm_elem, c.max_displacement);
                }
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
            }
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
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
        Some(s) => s.parse::<f64>().map_err(|_| format!("bad --h value: {s}"))?,
        None => 0.25,
    };
    if !h.is_finite() || h <= 0.0 {
        return Err("bad --h value: must be positive and finite".into());
    }
    let solid_only = args.iter().any(|a| a == "--solid");
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
    let tri = surface(
        |p: [f64; 3]| expr::eval(field_ref, p).v,
        [b.lo[0] - h, b.lo[1] - h, b.lo[2] - h],
        [b.hi[0] + h, b.hi[1] + h, b.hi[2] + h],
        h,
    );
    let mesh_s = (clock() - t0) / 1000.0;

    let t0 = clock();
    // Reference pair at the mesh spacing (export.rs in the wasm crate explains why the
    // coarser auto spacing's Richardson band misses the truth on TPMS fill fields).
    let v = volume::volume(field_ref, h);
    let ref_s = (clock() - t0) / 1000.0;
    let v_mesh = tri.volume();
    let delta = if v.volume > 0.0 { (v_mesh - v.volume) / v.volume * 100.0 } else { f64::NAN };

    let t0 = clock();
    let d = tri.defects();
    let scan_s = (clock() - t0) / 1000.0;

    println!(
        "mesh {} verts, {} tris at h={h} mm (mesh {:.2} s, defect scan {:.2} s)",
        tri.verts.len(),
        tri.tris.len(),
        mesh_s,
        scan_s
    );
    println!(
        "field volume {:.1} ± {:.1} mm³, mesh volume {:.1} mm³, Δ {:+.3} % (field ref {:.1} s)",
        v.volume, v.error, v_mesh, delta, ref_s
    );
    if d.none() {
        println!("defects: none (closed, 2-manifold, consistently oriented, self-intersection-free)");
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
    let mf_bytes = threemf.as_ref().map(|_| tri.write_3mf(&doc.name, &doc.material));
    for (path, bytes, what) in [
        stl.as_deref().zip(stl_bytes).map(|(p, b)| (p, b, "binary STL")),
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
    println!("export total {:.2} s (h {h:.3} mm)", (clock() - t_all) / 1000.0);
    Ok(())
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
        println!("| {} | {:.2} | {:.4} |", c.name, c.max_vm_elem, c.max_displacement);
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

/// `solve` on a document with a fill statement: the solid reference solve first, then the
/// graded re-solve with mass and class summary.
fn solve_graded(doc: &Document, res: &Resolution, report: bool) -> Result<(), String> {
    let (a, g) = graded(doc, *res, clock).map_err(|e| e.0)?;
    let p = &a.provenance;
    let fill = &g.spec;
    let (counts, rho) = graded_shape(&g);
    let n = (counts[0] + counts[1] + counts[2]) as f64;
    let lighter = if g.solid_mass_g > 0.0 { (1.0 - g.mass_g / g.solid_mass_g) * 100.0 } else { 0.0 };
    if report {
        println!("## solve report — {} (solid reference)\n", doc.name);
        println!(
            "P2 tetrahedra, element size {:.2} mm, {:.0} elements, {} DOF. Mesh {:.0} ms, solve {:.0} ms, {} iterations. Peaks are element-centroid von Mises.",
            p.element_size, p.elements, p.dofs, p.mesh_ms, p.solve_ms, p.iterations
        );
        print_case_table(&a.cases, "solid");
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
        println!("grade {:.0} ms, graded re-solve {:.0} ms", g.fill_ms, g.solve_ms);
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
            fill.family, fill.mode, fill.cell, g.mass_g, g.solid_mass_g, 100.0 * counts[1] as f64 / n
        );
        if counts[1] > 0 {
            println!("achieved design lattice ρ {:.2}..{:.2}, σ_ref {:.2} MPa", rho.0, rho.1, g.sigma_ref);
        }
        print_case_lines(&g.cases, "graded");
        println!("grade {:.0} ms, graded re-solve {:.0} ms", g.fill_ms, g.solve_ms);
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
