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
use fieldsmith_verify::{example, Check, Report};

/// 1 if two f64 differ at the bit level, else 0.
fn bit_diff(x: f64, y: f64) -> u64 {
    (x.to_bits() != y.to_bits()) as u64
}

/// Count f64 fields whose bits differ between two analyses (the fields
/// `assert_same_results` compares). 0 means bit-identical.
fn count_result_diffs(a: &Analysis, b: &Analysis) -> u64 {
    let mut n = 0u64;
    if a.provenance.elements != b.provenance.elements { n += 1; }
    if a.provenance.dofs != b.provenance.dofs { n += 1; }
    n += bit_diff(a.provenance.element_size, b.provenance.element_size);
    if a.cases.len() != b.cases.len() { n += 1; }
    for (ca, cb) in a.cases.iter().zip(b.cases.iter()) {
        if ca.name != cb.name { n += 1; }
        n += bit_diff(ca.max_von_mises, cb.max_von_mises);
        n += bit_diff(ca.max_vm_elem, cb.max_vm_elem);
        n += bit_diff(ca.max_layer_peel, cb.max_layer_peel);
        n += bit_diff(ca.max_displacement, cb.max_displacement);
        n += bit_diff(ca.compliance, cb.compliance);
        for (x, y) in ca.sigma_min_elem.iter().zip(cb.sigma_min_elem.iter()) { n += bit_diff(*x, *y); }
        for (x, y) in ca.peel_elem.iter().zip(cb.peel_elem.iter()) { n += bit_diff(*x, *y); }
        match (&ca.compliance_upper, &cb.compliance_upper) {
            (Some(x), Some(y)) => n += bit_diff(*x, *y),
            (None, None) => {}
            _ => n += 1,
        }
    }
    for (x, y) in a.envelope_elem.iter().zip(b.envelope_elem.iter()) { n += bit_diff(*x, *y); }
    for (x, y) in a.peel_envelope_elem.iter().zip(b.peel_envelope_elem.iter()) { n += bit_diff(*x, *y); }
    if a.build_angles != b.build_angles { n += 1; }
    if a.best_build_angles != b.best_build_angles { n += 1; }
    n += bit_diff(a.min_peak_peel, b.min_peak_peel);
    n
}

/// Count bit-differing fields between two graded fills (mirrors `assert_same_graded`).
fn count_graded_diffs(a: &Graded, b: &Graded) -> u64 {
    let mut n = 0u64;
    n += bit_diff(a.mass_g, b.mass_g);
    n += bit_diff(a.solid_mass_g, b.solid_mass_g);
    n += bit_diff(a.sigma_ref, b.sigma_ref);
    if a.rho_design != b.rho_design { n += 1; }
    if a.class != b.class { n += 1; }
    for (ca, cb) in a.cases.iter().zip(b.cases.iter()) {
        n += bit_diff(ca.max_von_mises, cb.max_von_mises);
        n += bit_diff(ca.max_displacement, cb.max_displacement);
        n += bit_diff(ca.compliance, cb.compliance);
        match (&ca.compliance_upper, &cb.compliance_upper) {
            (Some(x), Some(y)) => n += bit_diff(*x, *y),
            (None, None) => {}
            _ => n += 1,
        }
    }
    if a.yield_utilization.len() != b.yield_utilization.len() { n += 1; }
    for (x, y) in a.yield_utilization.iter().zip(b.yield_utilization.iter()) {
        match (x, y) {
            (Some(p), Some(q)) => n += bit_diff(*p, *q),
            (None, None) => {}
            _ => n += 1,
        }
    }
    n
}

/// Emit `docs/verification/V9.md`. Records the save → reload → re-solve bit-identity as
/// mismatch counts (0 = identical) at CI-budget resolutions; the sibling tests assert the
/// same property at 120 k DOF and panic on the first differing bit.
#[test]
fn v9_report() {
    let mut r = Report::new(
        "V9",
        "Save → Reload → Re-solve Bit-Identity (canonical .fsm round-trip)",
    );
    let res = Resolution::Dofs(8_000);

    // Solid bracket: print/parse fixed point + bit-identical re-solve.
    let solid = example("bracket.fsm");
    let reloaded = round_trip(&solid);
    let a1 = analyse(&solid, res, now).expect("solid analysis");
    let a2 = analyse(&reloaded, res, now).expect("solid analysis after reload");
    r.check(Check::absolute("Solid bracket re-solve bit mismatches", 0.0, count_result_diffs(&a1, &a2) as f64, "fields", 0.0));

    // Graded fill: bit-identical graded re-solve, including a rotated lattice.
    let filled = example("bracket-filled.fsm");
    let reloaded_filled = round_trip(&filled);
    let base1 = analyse(&filled, res, now).expect("filled solid analysis");
    let base2 = analyse(&reloaded_filled, res, now).expect("filled solid analysis after reload");
    let g1 = graded_of(&filled, &base1, now, false).expect("graded");
    let g2 = graded_of(&reloaded_filled, &base2, now, false).expect("graded after reload");
    r.check(Check::absolute("Graded fill re-solve bit mismatches", 0.0, count_graded_diffs(&g1, &g2) as f64, "fields", 0.0));
    let mut rotated = filled.clone();
    rotated.fill.as_mut().expect("fill").rotate = [15.0, 25.0, 45.0];
    let reloaded_rot = round_trip(&rotated);
    let gr1 = graded_of(&rotated, &base1, now, false).expect("graded rotated");
    let gr2 = graded_of(&reloaded_rot, &base2, now, false).expect("graded rotated after reload");
    r.check(Check::absolute("Rotated-lattice graded re-solve bit mismatches", 0.0, count_graded_diffs(&gr1, &gr2) as f64, "fields", 0.0));

    // Mesh document round-trips to an equal document (no field solve in this version).
    let mesh_doc = fieldsmith_doc::fsm::parse(
        "fieldsmith 1\n\
         name \"Import\"\n\
         material petg\n\
         mesh mesh1 src=\"/tmp/part.stl\" units=m up=y vox=0.8 hash=\"0123456789abcdef\" bbox=(0, 0, 0, 15.24, 17.25, 5) tris=87286 patches=6 open_edges=0\n\
         region bottom = mesh1.p3\n\
         support hold bottom fixed=xyz\n\
         load press bottom pressure=0.5 case=\"Load 1\"\n",
    )
    .expect("mesh document parses");
    let mesh_reloaded = round_trip(&mesh_doc);
    r.check(Check::absolute(
        "Mesh document save → reload reaches an equal document",
        1.0,
        if mesh_reloaded == mesh_doc && mesh_doc.validate().is_empty() { 1.0 } else { 0.0 },
        "-",
        0.0,
    ));

    // Equilibrated bracket: recovered upper end round-trips and brackets the lower end.
    let eq_res = Resolution::Dofs(6_000);
    let e1 = fieldsmith_core::analyse_full(&solid, eq_res, now, None, true).expect("equilibrated analysis");
    let e2 = fieldsmith_core::analyse_full(&reloaded, eq_res, now, None, true).expect("equilibrated analysis after reload");
    r.check(Check::absolute("Equilibrated bracket re-solve bit mismatches", 0.0, count_result_diffs(&e1, &e2) as f64, "fields", 0.0));
    let mut upper_ok = true;
    for (ca, cb) in e1.cases.iter().zip(e2.cases.iter()) {
        match (&ca.compliance_upper, &cb.compliance_upper) {
            (Some(x), Some(y)) => {
                if x.to_bits() != y.to_bits() || *x < ca.compliance - 1e-4 * ca.compliance {
                    upper_ok = false;
                }
            }
            _ => upper_ok = false,
        }
    }
    r.check(Check::absolute(
        "Recovered compliance upper end round-trips and brackets the lower end",
        1.0,
        if upper_ok { 1.0 } else { 0.0 },
        "-",
        0.0,
    ));

    r.section(
        "Summary",
        format!(
            "Save is the canonical document text (fsm::print), reload is fsm::parse, and \
             save → reload → save is a fixed point. At {} DOF the solid bracket, the graded \
             gyroid fill, and a rotated-lattice (15, 25, 45) fill all re-solve with 0 \
             bit-differing fields; the mesh document reloads to an equal document; and the \
             equilibrated bracket's recovered compliance upper end round-trips bit for bit \
             at 6 k DOF while staying above the conforming lower end. Wall-clock provenance \
             (mesh_ms, solve_ms) is excluded — it is not a property of the document.",
            8_000,
        ),
    );
    r.section(
        "Limits",
        "The report records bit-identity at CI-budget resolutions (8 k DOF solid/graded, \
         6 k DOF equilibrated); the sibling tests assert the same property at 120 k DOF and \
         panic on the first differing bit. Bit-identity is a property of this build's \
         deterministic solve on one machine; cross-machine fp divergence is measured \
         separately (V25, data/costs) and is not promised here. Mesh-node grids are compared \
         by hash in the V10 .fsmz tests, not by a field solve here.",
    );
    r.finish();
}

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
        // The most-compress-principal element values round-trip bit for bit (the
        // buckling indicator's load measure).
        assert_eq!(
            ca.sigma_min_elem
                .iter()
                .map(|x| x.to_bits())
                .collect::<Vec<_>>(),
            cb.sigma_min_elem
                .iter()
                .map(|x| x.to_bits())
                .collect::<Vec<_>>(),
            "{label}: {} min-compressive principal per element",
            ca.name
        );
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
            ca.max_layer_peel.to_bits(),
            cb.max_layer_peel.to_bits(),
            "{label}: {} max_layer_peel",
            ca.name
        );
        assert_eq!(
            ca.peel_elem.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
            cb.peel_elem.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
            "{label}: {} peel_elem",
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
    for (pa, pb) in a.peel_envelope_elem.iter().zip(b.peel_envelope_elem.iter()) {
        assert_eq!(pa.to_bits(), pb.to_bits(), "{label}: envelope element peel");
    }
    assert_eq!(a.build_angles, b.build_angles, "{label}: build_angles");
    assert_eq!(
        a.best_build_angles, b.best_build_angles,
        "{label}: best_build_angles"
    );
    assert_eq!(
        a.min_peak_peel.to_bits(),
        b.min_peak_peel.to_bits(),
        "{label}: min_peak_peel"
    );
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

    // Orthotropic layer-peel strengths (`yield_z`, `shear_z`) and `solve build=(theta, phi)`
    // round-trip in `.fsm` and reproduce the layer-peel analysis bit for bit.
    let mut ortho = solid.clone();
    ortho.custom_materials.push(fieldsmith_doc::Material {
        id: "petg_ortho".into(),
        name: "PETG Ortho".into(),
        e: 2100.0,
        nu: 0.36,
        density: 1270.0,
        yield_strength: 50.0,
        min_wall: 0.8,
        alpha: None,
        k: None,
        yield_z: Some(30.0),
        shear_z: Some(25.0),
        source: "test".into(),
        ..Default::default()
    });
    ortho.material = "petg_ortho".into();
    ortho.solve.build = Some([30.0, 15.0]);
    let printed_ortho = fieldsmith_doc::fsm::print(&ortho);
    assert!(
        printed_ortho.contains("yield_z=30 shear_z=25"),
        "expected yield_z=30 shear_z=25 in:\n{printed_ortho}"
    );
    assert!(
        printed_ortho.contains("build=(30, 15)"),
        "expected build=(30, 15) in:\n{printed_ortho}"
    );
    let reloaded_ortho = round_trip(&ortho);
    let ao1 = analyse(&ortho, Resolution::Dofs(15_000), now).expect("ortho analysis");
    let ao2 = analyse(&reloaded_ortho, Resolution::Dofs(15_000), now)
        .expect("ortho analysis after reload");
    assert_same_results(&ao1, &ao2, "ortho bracket with build=(30, 15)");
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

    // Rotated lattice fill (`rotate=(15, 25, 45)`) round-trips in `.fsm` and reproduces
    // the graded re-solve bit for bit.
    let mut rotated = filled.clone();
    rotated.fill.as_mut().expect("fill").rotate = [15.0, 25.0, 45.0];
    let printed_rot = fieldsmith_doc::fsm::print(&rotated);
    assert!(
        printed_rot.contains("rotate=(15, 25, 45)"),
        "expected rotate=(15, 25, 45) in:\n{printed_rot}"
    );
    let reloaded_rot = round_trip(&rotated);
    let gr1 = graded_of(&rotated, &base1, now, false).expect("graded rotated");
    let gr2 = graded_of(&reloaded_rot, &base2, now, false).expect("graded rotated after reload");
    assert_same_graded(&gr1, &gr2, "graded bracket with rotated lattice");
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
