//! V7 — the homogenised graded re-solve against a resolved lattice model.
//!
//! GOAL V7: a graded-lattice beam, resolved FE vs the homogenised re-solve, tip deflection
//! within 15 %.
//!
//! Beam: 80 × 16 × 16 mm steel cantilever (E = 200 000 MPa, clamp on the x = 0 face), 400 N
//! down on the tip face, filled with a gyroid-sheet lattice (cell 5 mm, skin 1 mm, ρ 0.2…0.4,
//! grading curve 2 driven by the load case's stress, min printable wall 0.3 mm). Two FE models
//! of the same authored part:
//!
//! * **Engine re-solve** (the user path): `graded` — a solid uniform mesh at element size
//!   1.0 mm (0.2 of the cell), one grading pass, then a graded re-solve of the same mesh and
//!   cases where each element's modulus comes from its sampled material fraction of the fill
//!   (φ, 32 jittered samples per element): Voigt mixing at sub-cell element sizes, blending
//!   over to the coupon-measured law by h/cell 1.5 (the mapping `fill.rs` documents; an
//!   earlier revision assigned lattice elements the law at their per-centroid design density
//!   capped at the band top, which read +21…+38 % too soft on this beam, non-monotone under
//!   mesh refinement — see `examples/v7_engine_probe.rs`).
//! * **Resolved model** (the reference): the exact graded field stuffed into a tet mesh, with
//!   the SOLID modulus on every element: every wall discretised, no homogenisation. Supports
//!   and loads go through the engine's own `fixed_mask` / `case_force`, so the two models
//!   share the boundary-condition code. Mesh sizes h = 0.33 and h = 0.29 mm.
//!
//! Design constraints the geometry has to satisfy for the comparison to mean anything:
//!
//! * **Inside the law's measured band at the design level.** The engine's grading drives the
//!   design density through the coupon laws, whose measured band is ρ ∈ [0.18, 0.40]
//!   (documented in `fill.rs`); the test asserts the achieved design range
//!   (`Graded::rho_design`, wall-thickening clamps included) stays inside it, so the design
//!   the engine grades is one the law actually measured. Sheet walls at ρmin need a cell big
//!   enough that `t(ρmin) = ρmin·cell / 3.09` stays above the authored min printable wall: at
//!   cell 5 and wall 0.3 mm, t(0.2) ≈ 0.32 mm, so the clamp does not bind. (An earlier draft
//!   with cell 4 drove every wall to the 0.8 mm default min-wall — achieved ρ ≈ 0.49, outside
//!   the band — and its engine read 23 % soft; that beam tested the clamp, not the law.)
//! * **Some cells through the beam depth.** Homogenisation needs scale separation: cell 5 in
//!   a 16 mm depth is 3.2 cells. Fully resolving this honestly sets the resolved mesh size:
//!   the thinnest walls are ~0.3 mm, so h ≈ t is the discretisation the law itself was
//!   measured with (V6: coupons at h = t, one P2 element per wall).
//!
//! Both models are reduced the same way: uz at the node nearest the tip centre (80, 8, 8),
//! plus max displacement (the engine's readout definition). A floating-fragment probe prunes
//! any disconnected blobs from the stuffed mesh (reported; the beam's fill is one body in the
//! runs below).
//!
//! Run (heavy, tens of minutes):
//! `cargo test -p fieldsmith-verify --release --test v7_graded_beam -- --ignored --nocapture`

use fieldsmith_core::fill::graded;
use fieldsmith_core::regions::Facet;
use fieldsmith_core::Material;
use fieldsmith_core::{case_force, fixed_mask, material_of, run_case, Resolution};
use fieldsmith_field::expr::{self, Expr};
use fieldsmith_mesh::{quality, stuff};
use fieldsmith_solve::p2::face_nodes;
use fieldsmith_solve::P2Mesh;
use fieldsmith_verify::coupon::keep_largest_connected;
use fieldsmith_verify::{doc, Check, Report};

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

/// One resolved-model run.
struct Resolved {
    h: f64,
    verts: usize,
    tets: usize,
    dofs: usize,
    min_dihedral: f64,
    max_dihedral: f64,
    ncomp: usize,
    dropped: usize,
    vol_mm3: f64,
    /// Tip deflection: uz at the node nearest the tip centre (mm).
    tip_uz: f64,
    tip_uz_node: [f64; 3],
    max_displacement: f64,
    iters: usize,
}

/// The P2 node nearest `target`.
fn nearest(mesh: &P2Mesh, target: [f64; 3]) -> usize {
    let (mut best, mut bd) = (0usize, f64::MAX);
    for (i, p) in mesh.nodes.iter().enumerate() {
        let d2 =
            (p[0] - target[0]).powi(2) + (p[1] - target[1]).powi(2) + (p[2] - target[2]).powi(2);
        if d2 < bd {
            bd = d2;
            best = i;
        }
    }
    best
}

#[test]
#[ignore = "heavy (tens of minutes): cargo test -p fieldsmith-verify --release --test v7_graded_beam -- --ignored --nocapture"]
fn v7_graded_lattice_beam_deflection() {
    let d = doc(DOC);
    let mat = material_of(&d).expect("material");
    let tip = [80.0, 8.0, 8.0];

    // ---- the engine path: solid solve, grade, graded re-solve ------------------------------
    let now = || std::time::Instant::now().elapsed().as_secs_f64() * 1000.0;
    let (base, g) = graded(&d, Resolution::Size(1.0), now).expect("graded analysis");
    let engine_case = g.cases.first().expect("at least one graded case");
    assert!(
        engine_case.solve.converged,
        "graded re-solve did not converge: {:?}",
        engine_case.solve
    );
    let solid_case = &base.cases[0];
    let solid_uz = solid_case.u[3 * nearest(&base.mesh, tip) + 2];
    let engine_tip_node = nearest(&base.mesh, tip);
    let engine_tip_uz = engine_case.u[3 * engine_tip_node + 2];
    let d_engine = engine_case.max_displacement;
    // The lattice must grade at all: the 1 mm solid skin carries much of the stress, but the
    // graded pockets still soften the beam well past a rounding margin.
    assert!(
        d_engine > solid_case.max_displacement * 1.3,
        "graded deflection {:.6} does not clearly exceed the solid beam's {:.6}: the fill did not grade",
        d_engine,
        solid_case.max_displacement
    );
    // All three element classes must appear (real lattice with skin and pockets).
    let (mut lat, mut voi) = (0usize, 0usize);
    for &k in &g.class {
        if k == fieldsmith_core::fill::CLASS_LATTICE {
            lat += 1;
        } else if k == fieldsmith_core::fill::CLASS_VOID {
            voi += 1;
        }
    }
    assert!(
        lat > 0 && voi > 0,
        "lattice {lat}, void {voi}: the fill did not grade"
    );
    // The design must sit inside the law's measured band: the achieved design density range
    // (wall-thickening clamps included) is what the engine grades, and the laws were measured
    // on ρ ∈ [0.18, 0.40]. 0.42 leaves a small margin over the authored 0.40.
    let (rho_lo, rho_hi) = (g.rho_design[0], g.rho_design[1]);
    assert!(
        rho_lo >= 0.17 && rho_hi <= 0.42,
        "achieved design density {rho_lo:.4}..{rho_hi:.4} strays outside the law's measured band [0.18, 0.40]",
    );

    // ---- the resolved model at two mesh sizes ----------------------------------------------
    let filled = Expr::Filled(Box::new(g.fill.clone()));
    let b = expr::bounds(&expr::compile_body(&d));
    assert!(!b.is_empty(), "empty model bounds");
    let case_name = engine_case.name.clone();

    let mut resolved: Vec<Resolved> = Vec::new();
    for &h in &[0.33, 0.29] {
        let f = |p: [f64; 3]| expr::eval(&filled, p).v;
        let (mut lin, _) = stuff(f, b.lo, b.hi, h);
        lin.compact();
        let (ncomp, dropped) = keep_largest_connected(&mut lin);
        let vol_mm3 = lin.volume();
        let q = quality(&lin);
        let mesh = P2Mesh::from_linear(&lin.verts, &lin.tets);
        let boundary = lin.boundary();
        let facets: Vec<Facet> = boundary
            .iter()
            .map(|(fc, ti)| {
                let tet = &mesh.tets[*ti as usize];
                let local = fc.map(|v| {
                    tet[..4]
                        .iter()
                        .position(|&x| x == v)
                        .expect("face vertex in tet")
                });
                Facet {
                    nodes: face_nodes(tet, local),
                }
            })
            .collect();
        let fixed = fixed_mask(&d, &mesh, &facets, h).expect("support touches the resolved mesh");
        let force =
            case_force(&d, &mesh, &facets, &case_name, h).expect("load touches the resolved mesh");
        let young = vec![mat.e; mesh.tets.len()];
        let case = run_case(
            &mesh,
            &Material::iso(young.to_vec(), mat.nu),
            &fixed,
            force,
            case_name.clone(),
            false,
            &|_| None,
        )
        .expect("resolved solve");
        assert!(
            case.solve.converged,
            "resolved solve h = {h} did not converge: {:?}",
            case.solve
        );
        let ti = nearest(&mesh, tip);
        let p = mesh.nodes[ti];
        resolved.push(Resolved {
            h,
            verts: lin.verts.len(),
            tets: lin.tets.len(),
            dofs: mesh.ndofs(),
            min_dihedral: q.min_dihedral_deg,
            max_dihedral: q.max_dihedral_deg,
            ncomp,
            dropped,
            vol_mm3,
            tip_uz: case.u[3 * ti + 2],
            tip_uz_node: p,
            max_displacement: case.max_displacement,
            iters: case.solve.iterations,
        });
    }

    let fines = resolved.last().expect("two resolved runs");
    let coarses = resolved.first().expect("two resolved runs");

    // ---- checks -----------------------------------------------------------------------------
    let mut r = Report::new(
        "V7",
        "Graded-lattice beam: homogenised re-solve vs resolved lattice FE (gyroid sheet, 400 N cantilever)",
    );
    // Reference = the resolved FE model (physical truth); measured = the engine's prediction.
    // (Check::relative(quantity, reference, measured, …): the error is measured relative to
    // the reference, so the resolved value goes first — swapping the slots silently turned
    // every error into |resolved − engine|/|engine| for the earlier run.)
    r.check(Check::relative(
        "tip deflection uz: engine re-solve against resolved FE at h = 0.29 (GOAL tolerance 15 %)",
        fines.tip_uz,
        engine_tip_uz,
        "mm",
        0.15,
    ));
    r.check(Check::relative(
        "tip deflection uz: engine re-solve against resolved FE at h = 0.33",
        coarses.tip_uz,
        engine_tip_uz,
        "mm",
        0.15,
    ));
    r.check(Check::relative(
        "max displacement: engine re-solve against resolved FE at h = 0.29",
        fines.max_displacement,
        d_engine,
        "mm",
        0.15,
    ));
    // Finer mesh as reference, coarser re-solve as the measurement it bounds.
    r.check(Check::relative(
        "resolved mesh study: tip deflection uz at h = 0.33 against the h = 0.29 reference",
        fines.tip_uz,
        coarses.tip_uz,
        "mm",
        0.10,
    ));
    // Soft cross-check between two independent volume evaluations of the same field (walls
    // ~0.32–0.64 mm against the field integration pitch 0.5 mm): catches gross integrator
    // disagreement, not a physical mass measurement. The mesh, finer-pitched through the
    // walls, is the reference.
    r.check(Check::relative(
        "fill mass: field integration against resolved mesh volume × density (h = 0.29)",
        fines.vol_mm3 * mat.density / 1000.0,
        g.mass_g,
        "g",
        0.20,
    ));

    // ---- sections ----------------------------------------------------------------------------
    r.section(
        "Model",
        format!(
            "Beam 80 × 16 × 16 mm at (40, 8, 8), steel (E = 200 000 MPa, ν = 0.3, yield \
250 MPa, density 7.85 g/cm³), cantilever clamp on the x = 0 face, 400 N down on the tip face \
(x = 80). Euler–Bernoulli tip deflection of the solid beam 0.0625 mm, root bending stress \
23 MPa — below yield, so both models deform elastically, and the short-beam shear term is a \
few percent. Fill: gyroid sheet, cell 5 mm (3.2 cells through the depth), skin 1 mm, \
ρ 0.2…0.4, grading curve 2 driven by the load case's stress, min printable wall 0.3 mm. The \
cell/wall pairing keeps the lattice inside the law's measured band: sheet walls from the \
calibration at ρmin are t = ρ·cell/3.09 ≈ 0.32 mm, above the authored minimum, so no min-wall \
clamp binds and the achieved design density {rho_lo:.3}…{rho_hi:.3} stays in the measured \
band — the earlier cell-4 configuration (all walls clamped to the 0.8 mm default, achieved \
ρ ≈ 0.49) tested the clamp, not the law, and is documented in the module docs. The engine path \
solves the solid part at element size 1.0 mm ({} dofs), grades one pass, and re-solves the \
case on the same mesh with per-element moduli from each element's sampled material fraction \
φ of the graded fill (32 jittered samples per element; Voigt mixing at this sub-cell element \
size, blending over to the coupon-measured law by h/cell 1.5). Graded region \
classes: {} lattice, {} void (pockets), {} solid (skin and protected). σ_ref (the stress at \
which ρmax is reached) {:.1} MPa. The resolved path stuffs the same graded field into tet \
meshes and gives every element the solid modulus; floating fragments are pruned and reported. \
Both paths call the same `fixed_mask` / `case_force` for the document's supports and loads.",
            base.mesh.ndofs(),
            lat,
            voi,
            g.class.len() - lat - voi,
            g.sigma_ref,
        ),
    );
    let res_lines: Vec<String> = resolved
        .iter()
        .map(|res| {
            format!(
                "h = {:.2}: {} verts, {} tets, {} dofs, dihedral {:.1}°…{:.1}°, {} component(s) \
({} floating tets dropped), volume {:.1} mm³; tip node at ({:.2}, {:.2}, {:.2}) reads uz \
{:.5} mm; max displacement {:.5} mm, {} iters",
                res.h,
                res.verts,
                res.tets,
                res.dofs,
                res.min_dihedral,
                res.max_dihedral,
                res.ncomp,
                res.dropped,
                res.vol_mm3,
                res.tip_uz_node[0],
                res.tip_uz_node[1],
                res.tip_uz_node[2],
                res.tip_uz,
                res.max_displacement,
                res.iters,
            )
        })
        .collect();
    r.section(
        "Deflection comparison",
        format!(
            "Solid beam (no fill, engine mesh): tip uz {:.5} mm. Engine re-solve (homogenised \
graded lattice): tip uz {:.5} mm, max displacement {:.5} mm, {} iters. The lattice multiplies \
tip deflection by {:.0} % in the engine model and by {:.0} % in the resolved model.\n\nThe engine re-solve reads {:+.1} % against the resolved reference at \
h = 0.29 and {:+.1} % at h = 0.33 (positive: the engine deflects the beam more, i.e. the \
homogenised model is softer). The resolved mesh study moves {:.1} % between h = 0.33 and \
h = 0.29, so the resolved reference's discretisation error is of that order.",
            solid_uz,
            engine_tip_uz,
            d_engine,
            engine_case.solve.iterations,
            100.0 * engine_tip_uz / solid_uz,
            100.0 * fines.tip_uz / solid_uz,
            // Signed vs the resolved reference: positive = the engine deflects more (predicts
            // a stiffer lattice… i.e. is too soft), negative = the engine deflects less.
            100.0 * (engine_tip_uz - fines.tip_uz) / fines.tip_uz,
            100.0 * (engine_tip_uz - coarses.tip_uz) / coarses.tip_uz,
            100.0 * (fines.tip_uz - coarses.tip_uz) / fines.tip_uz,
        ),
    );
    let res_body = res_lines.join("\n");
    r.section("Resolved model runs", &res_body);
    r.section(
        "Mass (grams)",
        format!(
            "Field-integrated (volume::integrate of the graded field, density 7.85): \
filled part {:.2} g, same part without fill {:.2} g — the lattice removes {:.0} % of the mass. \
Resolved mesh at h = 0.29: {:.2} g from mesh volume × density; the two filled estimates are \
independent discretisations of the same field and disagree by {:.1} % (the mesh value sampled \
the walls at pitch 0.29 mm, the field integral at ~{} mm).",
            g.mass_g,
            g.solid_mass_g,
            100.0 * (g.solid_mass_g - g.mass_g) / g.solid_mass_g,
            fines.vol_mm3 * mat.density / 1000.0,
            100.0 * (g.mass_g - fines.vol_mm3 * mat.density / 1000.0)
                / (fines.vol_mm3 * mat.density / 1000.0),
            (5.0f64 / 4.0).min(fieldsmith_field::volume::default_spacing(&b)),
        ),
    );
    r.section(
        "Coverage and limits",
        "One load case and one family/mode (gyroid sheet) are covered here; the other laws are \
the same engine code path, and their coupon measurements sit in the V6 harness history. The \
engine makes a single grading pass — densities are not re-iterated from the graded stress — \
so this verifies what one pass achieves, as the fill module documents. The element-modulus \
mapping is measured at this case's element size: a probe on the engine's own mesh and boundary \
conditions read it at −2.1 % (h = 1.0 mm) and −0.5 % (h = 0.75 mm) against the resolved \
reference below (`examples/v7_mf_probe.rs`). The blend from Voigt mixing to the homogenised \
law over h/cell ∈ [1, 1.5] is an interpolation between the two measured regimes, not itself a \
measurement; at 1.0 mm the blend weight is zero, so this run sits fully in the Voigt regime. \
The resolved walls at \
h ≈ 0.29–0.33 sit at the law's own measurement discretisation (V6 coupons at h = t, one P2 \
element per wall): the thinnest ρmin walls carry about one element across, the thickest \
(ρmax) about two — the resolved mesh study bounds the sensitivity this leaves in the \
reference. The mass check is a soft cross-check between two discretised integrations of the \
same field, not a physical measurement. Designs whose achieved lattice density falls outside \
the measured band [0.18, 0.40] (thin sheet walls clamped to the authored minimum printable \
wall) are governed by the clamp behaviour documented in `fill.rs`, not by this case.",
    );

    r.finish();
}
