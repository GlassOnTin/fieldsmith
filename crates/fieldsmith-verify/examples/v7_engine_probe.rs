//! Scratch probe for V7's engine-path softness (not a GOAL case).
//!
//! Engine sweeps so far on the fixed 80 × 16 × 16 gyroid-sheet document (resolved truth tip
//! uz −0.13223 mm at h = 0.29/0.33, mesh study 0.2 %):
//!
//! * Element ≪ cell (1.00 / 0.75 / 0.50 mm): −0.18180 / −0.16033 / −0.16964 → +37.6 % /
//!   +21.3 % / +28.3 % too soft, NON-MONOTONE. Refinement does not converge. Cause (fill.rs
//!   classification): the engine assigns each element's modulus from its centroid — centroid
//!   inside a wall reads the law capped at ρ 0.4 (≤ 0.167·E for solid steel), centroid in a
//!   pocket reads void. Right only when elements span ≥ 1 cell (homogenised regime), so at
//!   element ≪ cell thin walls are systematically mis-rendered and refinement can't fix it.
//! * This run sweeps element ≥ cell (2.5 / 5.0 / 10.0 mm = ½/1/2 cells per element), where
//!   the centroid rule is its intended semantics. Competing bias at these sizes: the solid
//!   skin (1 mm) mixes into boundary elements and the outer geometry is staircase-meshed, so
//!   each size also reports the SOLID solve's tip uz (solid at Size 1.0 was +0.8 % vs
//!   EB + shear ≈ 0.0631 mm) to expose geometric coarsening separately.
//!
//! Run: `cargo run -p fieldsmith-verify --release --example v7_engine_probe`

use fieldsmith_core::fill::graded;
use fieldsmith_core::Resolution;

const DOC: &str = "\
name \"V7 graded beam\"
custommaterial steel200 e=200000 nu=0.3 yield=250 density=7.85
material steel200
box beam size=(80, 16, 16) at=(40, 8, 8)
region cap = beam.nx
region tip = beam.px
support clamp cap fixed=xyz
load bend tip force=(0, 0, -400)
fill gyroid sheet cell=5 skin=1 rho=(0.2, 0.4) curve=2 wall=0.3
";

fn tip_uz(u: &[f64], mesh: &fieldsmith_solve::P2Mesh, target: [f64; 3]) -> f64 {
    let (mut best, mut bd) = (0usize, f64::MAX);
    for (i, p) in mesh.nodes.iter().enumerate() {
        let d2 = (p[0] - target[0]).powi(2)
            + (p[1] - target[1]).powi(2)
            + (p[2] - target[2]).powi(2);
        if d2 < bd {
            bd = d2;
            best = i;
        }
    }
    u[3 * best + 2]
}

fn main() {
    let d = fieldsmith_verify::doc(DOC);
    let now = || std::time::Instant::now().elapsed().as_secs_f64() * 1000.0;
    println!("resolved truth (h = 0.29/0.33, engine-fixed doc): tip uz -0.13223 mm");
    for &h in &[2.5, 5.0, 10.0] {
        let t0 = now();
        let (base, g) = graded(&d, Resolution::Size(h), now).expect("graded analysis");
        let c = g.cases.first().expect("case");
        let uz = tip_uz(&c.u, &base.mesh, [80.0, 8.0, 8.0]);
        let suz = tip_uz(&base.cases[0].u, &base.mesh, [80.0, 8.0, 8.0]);
        let rho: Vec<f64> = g.rho_elem.iter().copied().filter(|&r| r > 0.0 && r < 1.0).collect();
        let (lo, mean, hi) = (
            rho.iter().cloned().fold(f64::INFINITY, f64::min),
            rho.iter().sum::<f64>() / rho.len() as f64,
            rho.iter().cloned().fold(0.0, f64::max),
        );
        let lat = g.class.iter().filter(|&&k| k == 1).count();
        let err = 100.0 * (uz + 0.13223) / -0.13223;
        println!(
            "engine Size({h:.2}): dofs {}, solid tip uz {suz:.5}, graded tip uz {uz:.5} \
({err:+.1} % vs resolved), max disp {:.5}, iters {}, \
achieved rho {lo:.3}..{hi:.3} (mean {mean:.3}), lattice elems {lat}, mass {:.2} g, t {t0:.0} s",
            base.mesh.ndofs(),
            c.max_displacement,
            c.solve.iterations,
            g.mass_g,
        );
    }
}