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

use fieldsmith_core::{analyse, Resolution};
use fieldsmith_doc::{fsm, Document};
use fieldsmith_field::{expr, glsl, volume};
use std::process::ExitCode;

const USAGE: &str = "usage: fieldsmith <check|print|json|volume|glsl|solve> FILE.fsm [options]

  check   parse and validate a document
  print   print the canonical .fsm form
  json    print the document as JSON
  volume  integrate the field volume   [--h <spacing mm>]
  glsl    print the generated preview shader
  solve   mesh and solve              [--h <mm> | --dofs <n>] [--report]";

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
            let a = analyse(&doc, res, clock).map_err(|e| e.0)?;
            let p = &a.provenance;
            if args.iter().any(|a| a == "--report") {
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
