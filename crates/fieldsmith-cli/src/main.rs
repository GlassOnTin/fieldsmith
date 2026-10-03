//! `fieldsmith` — run any Fieldsmith stage headless.
//!
//! ```text
//! fieldsmith check  FILE.fsm            parse + validate, print a summary
//! fieldsmith print  FILE.fsm            canonical .fsm on stdout
//! fieldsmith json   FILE.fsm            document as JSON
//! fieldsmith volume FILE.fsm [--h MM]   field volume with error estimate
//! fieldsmith glsl   FILE.fsm            generated preview shader
//! fieldsmith solve  FILE.fsm            mesh and solve the current setup
//! ```

use fieldsmith_core::fill::graded;
use fieldsmith_core::{analyse, CaseResult, Resolution};
use fieldsmith_doc::{fsm, Document};
use fieldsmith_field::{expr, glsl, volume};
use std::process::ExitCode;

const USAGE: &str = "usage: fieldsmith <check|print|json|volume|glsl|solve> FILE.fsm [options]

  check   parse and validate a document
  print   print the canonical .fsm form
  json    print the document as JSON
  volume  integrate the field volume   [--h <spacing mm>]
  glsl    print the generated preview shader
  solve   mesh and solve               [--h <mm> | --dofs <n>] [--report]
          (documents with a fill statement also get the graded re-solve)";

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
    let (a, g) = graded(doc, res.clone(), clock).map_err(|e| e.0)?;
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
