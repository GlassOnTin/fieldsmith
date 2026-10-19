//! V9 — save → reload → re-solve of the bracket is bit-identical.
//!
//! GOAL step 10 / V9: save the document as `.fsm`, reload it, get identical results. The
//! save is the canonical document text (`fsm::print`), the reload is `fsm::parse`, and the
//! results are the analysis (and, for the filled document, the graded re-solve). Bit-identical
//! means the f64 results agree at the bit level; wall-clock provenance (mesh_ms, solve_ms) is
//! excluded — it is not a property of the document.

use fieldsmith_core::fill::{graded_of, Graded};
use fieldsmith_core::{analyse, Analysis, Resolution};
use fieldsmith_doc::Document;
use fieldsmith_verify::example;

fn now() -> f64 {
    std::time::Instant::now().elapsed().as_secs_f64() * 1000.0
}

/// Bit-compare everything except timing provenance. Panics with the first mismatch.
fn assert_same_results(a: &Analysis, b: &Analysis, label: &str) {
    assert_eq!(
        a.provenance.elements, b.provenance.elements,
        "{label}: elements"
    );
    assert_eq!(a.provenance.dofs, b.provenance.dofs, "{label}: dofs");
    assert_eq!(
        a.provenance.element_size.to_bits(),
        b.provenance.element_size.to_bits(),
        "{label}: element size"
    );
    assert_eq!(a.cases.len(), b.cases.len(), "{label}: case count");
    for (ca, cb) in a.cases.iter().zip(b.cases.iter()) {
        assert_eq!(ca.name, cb.name, "{label}: case name");
        assert_eq!(
            ca.max_von_mises.to_bits(),
            cb.max_von_mises.to_bits(),
            "{label}: {} nodal vm",
            ca.name
        );
        assert_eq!(
            ca.max_vm_elem.to_bits(),
            cb.max_vm_elem.to_bits(),
            "{label}: {} elem vm",
            ca.name
        );
        assert_eq!(
            ca.max_displacement.to_bits(),
            cb.max_displacement.to_bits(),
            "{label}: {} deflection",
            ca.name
        );
        assert_eq!(
            ca.compliance.to_bits(),
            cb.compliance.to_bits(),
            "{label}: {} raw-force compliance",
            ca.name
        );
        match (&ca.compliance_upper, &cb.compliance_upper) {
            (None, None) => {}
            (Some(x), Some(y)) => assert_eq!(
                x.to_bits(),
                y.to_bits(),
                "{label}: {} recovered-stress upper bound",
                ca.name
            ),
            _ => panic!(
                "{label}: {} compliance_upper present in one analysis only",
                ca.name
            ),
        }
    }
    for (ea, eb) in a.envelope_elem.iter().zip(b.envelope_elem.iter()) {
        assert_eq!(ea.to_bits(), eb.to_bits(), "{label}: envelope element vm");
    }
}

fn assert_same_graded(a: &Graded, b: &Graded, label: &str) {
    assert_eq!(a.mass_g.to_bits(), b.mass_g.to_bits(), "{label}: mass");
    assert_eq!(
        a.solid_mass_g.to_bits(),
        b.solid_mass_g.to_bits(),
        "{label}: solid mass"
    );
    assert_eq!(
        a.sigma_ref.to_bits(),
        b.sigma_ref.to_bits(),
        "{label}: sigma_ref"
    );
    assert_eq!(a.rho_design, b.rho_design, "{label}: rho_design");
    assert_eq!(a.class, b.class, "{label}: classes");
    for (ca, cb) in a.cases.iter().zip(b.cases.iter()) {
        assert_eq!(
            ca.max_von_mises.to_bits(),
            cb.max_von_mises.to_bits(),
            "{label}: graded vm"
        );
        assert_eq!(
            ca.max_displacement.to_bits(),
            cb.max_displacement.to_bits(),
            "{label}: graded deflection"
        );
        assert_eq!(
            ca.compliance.to_bits(),
            cb.compliance.to_bits(),
            "{label}: graded raw-force compliance"
        );
        match (&ca.compliance_upper, &cb.compliance_upper) {
            (None, None) => {}
            (Some(x), Some(y)) => assert_eq!(
                x.to_bits(),
                y.to_bits(),
                "{label}: graded recovered-stress upper bound"
            ),
            _ => panic!("{label}: graded compliance_upper mismatch"),
        }
    }
    // The yield-utilization factors (per case; absent — None — without a yield-stating
    // material) round-trip bit for bit with the solve.
    assert_eq!(
        a.yield_utilization.len(),
        b.yield_utilization.len(),
        "{label}: utilization case count"
    );
    for (i, (ux, uy)) in a
        .yield_utilization
        .iter()
        .zip(b.yield_utilization.iter())
        .enumerate()
    {
        match (ux, uy) {
            (None, None) => {}
            (Some(p), Some(q)) => assert_eq!(
                p.to_bits(),
                q.to_bits(),
                "{label}: case {i} yield-utilization factor"
            ),
            _ => panic!("{label}: case {i} utilization present in one graded run only"),
        }
    }
}

fn round_trip(doc: &Document) -> Document {
    let printed = fieldsmith_doc::fsm::print(doc);
    let reloaded = fieldsmith_doc::fsm::parse(&printed)
        .unwrap_or_else(|e| panic!("the saved document must parse: {}{}", e.line, e.message));
    let again = fieldsmith_doc::fsm::print(&reloaded);
    assert_eq!(again, printed, "save → reload → save is a fixed point");
    reloaded
}

#[test]
fn save_reload_resolve_is_bit_identical() {
    let res = Resolution::Dofs(120_000);

    let solid = example("bracket.fsm");
    let reloaded = round_trip(&solid);
    let a1 = analyse(&solid, res, now).expect("solid analysis");
    let a2 = analyse(&reloaded, res, now).expect("solid analysis after reload");
    assert_same_results(&a1, &a2, "solid bracket");
}

#[test]
fn save_reload_graded_resolve_is_bit_identical() {
    let res = Resolution::Dofs(120_000);

    let filled = example("bracket-filled.fsm");
    let reloaded = round_trip(&filled);
    let base1 = analyse(&filled, res, now).expect("solid analysis");
    let base2 = analyse(&reloaded, res, now).expect("solid analysis after reload");
    assert_same_results(&base1, &base2, "solid reference of the filled bracket");

    let g1 = graded_of(&filled, &base1, now, false).expect("graded");
    let g2 = graded_of(&reloaded, &base2, now, false).expect("graded after reload");
    assert_same_graded(&g1, &g2, "graded bracket");
}

/// A document with a mesh node round-trips with every mesh field and the patch selector
/// intact (M6 step 4). The node's provenance travels in the `.fsm` text, so equality of the
/// documents — not of an analysis (mesh nodes are not field leaves in this version) — is
/// what save/reload must guarantee here; the grids themselves are compared by hash in the
/// V10 `.fsmz` tests.
#[test]
fn save_reload_mesh_document_reaches_the_same_document() {
    let text = "fieldsmith 1\n\
name \"Import\"\n\
material petg\n\
\n\
mesh mesh1 src=\"/tmp/part.stl\" units=m up=y vox=0.8 hash=\"0123456789abcdef\" bbox=(0, 0, 0, 15.24, 17.25, 5) tris=87286 patches=6 open_edges=0\n\
\n\
region bottom = mesh1.p3\n\
support hold bottom fixed=xyz\n\
load press bottom pressure=0.5 case=\"Load 1\"\n";
    let doc = fieldsmith_doc::fsm::parse(text).expect("mesh document parses");
    assert!(doc.validate().is_empty(), "{:?}", doc.validate());

    let reloaded = round_trip(&doc);
    assert_eq!(
        reloaded, doc,
        "the reloaded document must equal the saved one"
    );
}

/// The equilibrated bracket round-trips: with the recovery's flag on, save → reload →
/// re-solve reproduces C(σ*) bit for bit at a resolution the direct sparse solve
/// finishes inside CI budget. The recovered ends are certified up to the witness slack
/// (direct-solve arithmetic, measured ~1e-6 relative in-span), so the ordering assert
/// carries that slack; the round-trip comparison itself stays bit-exact.
#[test]
fn save_reload_equilibrated_bracket_is_bit_identical() {
    let res = Resolution::Dofs(6_000);

    let solid = example("bracket.fsm");
    let reloaded = round_trip(&solid);
    let a1 =
        fieldsmith_core::analyse_full(&solid, res, now, None, true).expect("equilibrated analysis");
    let a2 = fieldsmith_core::analyse_full(&reloaded, res, now, None, true)
        .expect("equilibrated analysis after reload");
    assert_same_results(&a1, &a2, "equilibrated bracket");
    for (ca, cb) in a1.cases.iter().zip(a2.cases.iter()) {
        let (Some(x), Some(y)) = (&ca.compliance_upper, &cb.compliance_upper) else {
            panic!(
                "{}: the flag was set but the upper end is absent (converged: {})",
                ca.name, ca.solve.converged
            );
        };
        assert_eq!(
            x.to_bits(),
            y.to_bits(),
            "{}: the recovered upper end round-trips",
            ca.name
        );
        assert!(
            *x >= ca.compliance - 1e-4 * ca.compliance,
            "{}: bracket ordering, upper {x} vs lower {}",
            ca.name,
            ca.compliance
        );
    }
}
