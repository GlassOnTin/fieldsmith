//! Probe: material-fraction element moduli, emulated on the engine's own mesh and BCs
//! (scratch — not a GOAL case; it tested the candidate fix for the V7 engine defect before
//! the engine adopted it).
//!
//! Established this arc on the fixed 80×16×16 gyroid-sheet document (resolved truth tip
//! uz −0.13223 mm, mesh study 0.2 %): the engine's per-centroid classification has no honest
//! uniform element size for thin-skin fills. Element ≪ cell caps wall elements at
//! law(0.4) ≈ 0.167·E (+21…+38 % too soft, non-monotone in h); element ≥ cell void-ifies the
//! sub-skin boundary band (+40 % at 2.5 mm, +140 % at 5 mm, floppy at 10 mm). The solid solve
//! stays within ~1 % of beam theory at every size, so geometry is not the problem.
//!
//! Fix emulated here: per-element modulus from the element's MATERIAL FRACTION φ of
//! the actual fill field (jitter-sampled), two mappings:
//! * `phi-voigt` — young = E·φ: exact at φ=0/1, Voigt upper bound in between; the bending-
//!   consistent sub-cell answer (wall slabs carry E over their fraction, parallel-axis).
//! * `phi-law-blend` — law(φ) below the band top 0.4, linear blend from law(0.4) to 1 at
//!   φ=1: keeps the measured law where it was measured, heals only the capped-solid defect.
//!
//! The engine's old law path is re-run in the same process for a like-for-like column.
//!
//! Final three-point results (tip uz vs resolved −0.13223): phi-voigt −2.1 % / −0.5 % / +0.8 %
//! at engine sizes 1.0 / 0.75 / 0.5 mm; phi-law-blend −0.8 % / −1.9 % / −3.5 %. The engine
//! ships the Voigt end (sub-cell h/cell < 1), blending to the law only above h/cell 1. Run
//! cost at 0.5 mm: ~8.3M dofs, 4738 s, 57 GB RSS.
//!
//! Run: `cargo run -p fieldsmith-verify --release --example v7_mf_probe`

use fieldsmith_core::fill::{graded, law_stiffness};
use fieldsmith_doc::{Family, FillMode};
use fieldsmith_core::{case_force, fixed_mask, material_of, run_case, Resolution};
use fieldsmith_field::expr::{self, Expr};
use fieldsmith_solve::P2Mesh;

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

/// The P2 node nearest `target`.
fn nearest(mesh: &P2Mesh, target: [f64; 3]) -> usize {
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
    best
}

/// Deterministic LCG for jittered point sampling (same sequence every run).
struct Lcg(u64);
impl Lcg {
    fn next_f64(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / (1u64 << 53) as f64
    }
}

/// Element material fraction φ by jittered sampling (Dirichlet(1) barycentric points).
fn phi_of(base: &fieldsmith_core::Analysis, e: usize, filled: &Expr, samples: u32, rng: &mut Lcg) -> f64 {
    let tet = base.mesh.tets[e];
    let v = [tet[0], tet[1], tet[2], tet[3]].map(|n| base.mesh.nodes[n as usize]);
    let mut inside = 0u32;
    let mut lam = [0.0f64; 4];
    for _ in 0..samples {
        let mut s = 0.0;
        for l in &mut lam {
            *l = -(rng.next_f64() + 1e-12).ln();
            s += *l;
        }
        let mut p = [0.0f64; 3];
        for c in 0..3 {
            p[c] = (lam[0] * v[0][c] + lam[1] * v[1][c] + lam[2] * v[2][c] + lam[3] * v[3][c]) / s;
        }
        if expr::eval(filled, p).v < 0.0 {
            inside += 1;
        }
    }
    inside as f64 / samples as f64
}

/// young = E·(VOID_SCALE + φ·(1−VOID_SCALE)) — the Voigt-style candidate, with the engine's
/// tiny void floor so the stiffness matrix stays connected.
fn voigt_young(ph: &[f64], mat_e: f64) -> Vec<f64> {
    ph.iter().map(|&p| mat_e * (1e-6 + p * (1.0 - 1e-6))).collect()
}

/// young = E·law(φ) for φ ≤ 0.4, linear blend law(0.4)→1 over φ ∈ (0.4, 1].
fn law_blend_young(ph: &[f64], mat_e: f64) -> Vec<f64> {
    let s_hi = law_stiffness(Family::Gyroid, FillMode::Sheet, 0.4);
    ph.iter()
        .map(|&p| {
            let s = if p <= 0.4 {
                law_stiffness(Family::Gyroid, FillMode::Sheet, p)
            } else {
                s_hi + (p - 0.4) / 0.6 * (1.0 - s_hi)
            };
            mat_e * (1e-6 + s * (1.0 - 1e-6))
        })
        .collect()
}

fn main() {
    let d = fieldsmith_verify::doc(DOC);
    let mat = material_of(&d).expect("material");
    let now = || std::time::Instant::now().elapsed().as_secs_f64() * 1000.0;
    let resolved = -0.13223;
    println!("resolved truth (h = 0.29/0.33, engine-fixed doc): tip uz {resolved} mm");
    for &h in &[1.0, 0.75, 0.5] {
        let t0 = std::time::Instant::now();
        let (base, g) = graded(&d, Resolution::Size(h), now).expect("graded analysis");
        let case_name = g.cases.first().expect("case").name.clone();
        let law_uz = g.cases.first().unwrap().u[3 * nearest(&base.mesh, [80.0, 8.0, 8.0]) + 2];
        let filled = Expr::Filled(Box::new(g.fill.clone()));
        let facets: Vec<fieldsmith_core::regions::Facet> = base
            .surface
            .iter()
            .map(|&nodes| fieldsmith_core::regions::Facet { nodes })
            .collect();
        let fixed = fixed_mask(&d, &base.mesh, &facets, h).expect("support touches");
        let force = case_force(&d, &base.mesh, &facets, &case_name, h).expect("load touches");

        // φ per element (32 jittered samples), then both modulus mappings.
        let mut rng = Lcg(0x9E37_79B9_7F4A_7C15);
        let ph: Vec<f64> = (0..base.mesh.tets.len())
            .map(|e| phi_of(&base, e, &filled, 32, &mut rng))
            .collect();
        let (n_void, n_solid, n_lat) = ph.iter().fold((0, 0, 0), |a, &p| {
            (a.0 + (p < 0.02) as usize, a.1 + (p > 0.98) as usize, a.2 + (0.02..=0.98).contains(&p) as usize)
        });
        let t_ph = t0.elapsed().as_secs_f64();

        // Solve φ-Voigt and φ-law-blend on the engine's mesh.
        let mut out = Vec::new();
        for (label, young) in [
            ("phi-voigt", voigt_young(&ph, mat.e)),
            ("phi-law-blend", law_blend_young(&ph, mat.e)),
        ] {
            let case = run_case(&base.mesh, &young, mat.nu, &fixed, force.clone(), label.to_string())
                .expect("phi solve");
            let uz = case.u[3 * nearest(&base.mesh, [80.0, 8.0, 8.0]) + 2];
            let err = 100.0 * (uz - resolved) / resolved;
            out.push(format!("{label}: tip uz {uz:.5} ({err:+.1} % vs resolved), iters {}", case.solve.iterations));
        }
        let law_err = 100.0 * (law_uz - resolved) / resolved;
        println!(
            "Size({h:.2}): dofs {}, engine-law tip uz {law_uz:.5} ({law_err:+.1} %), φ classes \
void {n_void} / lattice {n_lat} / solid {n_solid}, φ sampling {t_ph:.0} s, total {:.0} s",
            base.mesh.ndofs(),
            t0.elapsed().as_secs_f64(),
        );
        for line in out {
            println!("    {line}");
        }
    }
}