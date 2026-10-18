//! The versioned headless JSON API (GOAL v0.4 section C, slice 1): one request in, one
//! envelope out. [`respond`] is the entry point — the one-shot `fieldsmith api` verb
//! feeds it stdin and prints the body; the MCP layer (slice 2) calls the same functions.
//!
//! Rules the envelope keeps:
//! - `ok` and `refusal` both exit 0 at the one-shot (a refusal is an answer); errors of
//!   processing are a JSON `error` status, and the CLI maps that to exit code 2.
//! - [`question::run`] is the ONE verified ladder loop; the CLI is a [`question::Sink`]
//!   implementation over it and the envelope accumulates its records through the same
//!   runner.
//! - The API reads the costs store for seeding (see [`store`]) and never writes it —
//!   row writes stay with the CLI (and later the worker host).

pub mod b64;
pub mod export;
pub mod question;
pub mod schema;
pub mod store;
pub mod sweep_csv;

use fieldsmith_core::rung::Tolerance;
use fieldsmith_core::{analyse, analyse_modes, fsmz, Resolution, SetupError};
use fieldsmith_doc::{fsm, param, Document, NodeKind};
use fieldsmith_field::expr;
use fieldsmith_field::meshfield::{key_from_hex, lookup};

use schema::{QuestionSpec, Request, Response, Status, APIV};

/// One answer: the compact JSON on one line (no trailing newline — stdout adds one)
/// plus whether it is a processing error (`status: error`), which the one-shot maps to
/// exit code 2. Ok and refusal are both `is_error: false`.
pub struct Reply {
    pub body: String,
    pub is_error: bool,
    /// The typed response behind `body`, with the engine's own f64s. Anything that
    /// re-derives numbers from `body` after a serde parse drifts: serde_json's float
    /// parser is not correctly rounded — measured 2026-10-14, the envelope literal
    /// `0.39066861588109364` parses back 1 ulp low (`0x3fd900b6f0321ac2` vs the
    /// correctly rounded `0x3fd900b6f0321ac3`), and its printed shortest form changes
    /// with it. The CSV builder reads this field so its cells print the same digits
    /// the wire envelope carries (and the same a correctly rounding client computes).
    pub response: Option<Response>,
}

/// The request's document: exactly one of `.fsm` text or base64 `.fsmz` bytes, parsed
/// through the same parser the CLI uses. The error strings are part of the contract
/// (the V22 golden asserts them), not prose.
pub fn load_doc(doc: Option<&str>, docz: Option<&str>) -> Result<Document, String> {
    match (doc, docz) {
        (Some(_), Some(_)) => {
            Err("doc: pass exactly one of doc (.fsm text) or docz (base64 .fsmz) — got both".into())
        }
        (None, None) => {
            Err("request carries no document — pass doc (.fsm text) or docz (base64 .fsmz)".into())
        }
        (Some(text), _) => {
            fsm::parse(text).map_err(|e| format!("doc: line {}: {}", e.line, e.message))
        }
        (_, Some(z)) => {
            let bytes = b64::decode(z).map_err(|e| format!("docz: {e}"))?;
            fsmz::load(&bytes)
                .map(|l| l.doc)
                .map_err(|e| format!("docz: {e}"))
        }
    }
}

/// Hashes of the document's mesh nodes whose voxel grid is not in this process's
/// registry. A question asked against missing grids would silently evaluate an
/// all-outside field, so [`respond`] refuses and document-openers (the MCP) check
/// with the same read.
pub fn missing_grids(doc: &Document) -> Vec<String> {
    doc.root
        .iter()
        .filter_map(|n| match &n.kind {
            NodeKind::Mesh { hash, .. } => Some(hash.clone()),
            _ => None,
        })
        .filter(|h| lookup(key_from_hex(h)).is_none())
        .collect()
}

/// Turn one JSON request into one JSON envelope.
pub fn respond(input: &str) -> Reply {
    // 1. Parse. Before the request is understood there is nothing to echo.
    let req: Request = match serde_json::from_str(input) {
        Ok(r) => r,
        Err(e) => return error_reply(APIV, None, None, Vec::new(), format!("request: {e}")),
    };

    // 2. Version. A mismatch is refused, not translated, and names the change — the
    // caller reads what differs from the engine's own line.
    if req.apiv != APIV {
        return error_reply(
            req.apiv,
            None,
            Some(&req.question),
            Vec::new(),
            format!(
                "apiv: request declares {}, this engine speaks {APIV} — apiv 2 added \
                 the sweep question (v1 requests are not translated; see schema.rs)",
                req.apiv
            ),
        );
    }

    // 3–4. Load the document (the shared loader the document-openers use too).
    let doc: Document = match load_doc(req.doc.as_deref(), req.docz.as_deref()) {
        Ok(d) => d,
        Err(message) => return error_reply(APIV, None, Some(&req.question), Vec::new(), message),
    };

    let name = doc.name.clone();
    let mut warnings = doc.material_warnings();

    // 5. Grids. Only the inspection verbs work without the registry; every question
    // here evaluates the body, which would silently see an all-outside field.
    let missing = missing_grids(&doc);
    if !missing.is_empty() {
        return error_reply(
            APIV,
            Some(&name),
            Some(&req.question),
            warnings,
            format!(
                "mesh grid{} {} {} not loaded — a `doc` (.fsm) carries no grids; pass \
                 docz (the base64 .fsmz sidecar), or import the source again in this \
                 process first",
                if missing.len() == 1 { "" } else { "s" },
                missing.join(", "),
                if missing.len() == 1 { "is" } else { "are" }
            ),
        );
    }

    // 6. Answer the question. `echo` is what every late validation error and the
    // final envelope carry back.
    let echo = req.question.clone();
    match req.question {
        QuestionSpec::Ladder {
            rungs,
            tolerance,
            budget,
        } => {
            if doc.fill.is_some() {
                return error_reply(
                    APIV,
                    Some(&name),
                    Some(&echo),
                    warnings,
                    "the ladder runs the solid path — remove the fill statement to use \
                     the declared-question modes",
                );
            }
            let rungs = match rungs {
                None => question::DEFAULT_RUNGS.to_vec(),
                Some(v) => match question::validate_rungs(&v) {
                    Ok(()) => v,
                    Err(e) => return error_reply(APIV, Some(&name), Some(&echo), warnings, e),
                },
            };
            let tolerance = match tolerance {
                None => None,
                Some(s) => match Tolerance::parse(&s) {
                    Ok(t) => Some((t, s.clone())),
                    Err(e) => {
                        return error_reply(APIV, Some(&name), Some(&echo), warnings, e);
                    }
                },
            };
            let budget_ms = match budget {
                None => None,
                Some(s) => match question::parse_budget(&s) {
                    Ok(b) => Some(b * 1000.0),
                    Err(e) => {
                        return error_reply(
                            APIV,
                            Some(&name),
                            Some(&echo),
                            warnings,
                            format!("budget {s:?}: {e}"),
                        );
                    }
                },
            };
            let machine = store::Machine::probe();
            // Seeding only: the API reads the store, it never writes it.
            let store_rows = store::store_path()
                .map(|p| store::read_all(&p))
                .unwrap_or_default();
            let q = question::Question {
                rungs,
                tolerance,
                budget_ms,
                store_rows,
                machine,
                clock: question::clock,
            };
            let mut sink = Collect;
            match question::run(&doc, &q, &mut sink) {
                Ok(o) => {
                    let status = if o.unmet.is_some() {
                        Status::Refusal
                    } else {
                        Status::Ok
                    };
                    ok_reply(schema::Response {
                        apiv: APIV,
                        engine: store::engine_identity(),
                        status,
                        name: Some(name),
                        question: Some(echo),
                        warnings,
                        outcome: Some(outcome_json(&doc, o)),
                        single: None,
                        sweep: None,
                        error: None,
                    })
                }
                Err(SetupError(e)) => error_reply(APIV, Some(&name), Some(&echo), warnings, e),
            }
        }
        QuestionSpec::Solve { dofs, h } => {
            if doc.fill.is_some() {
                return error_reply(
                    APIV,
                    Some(&name),
                    Some(&echo),
                    warnings,
                    "the api runs the solid path — remove the fill statement",
                );
            }
            let res = match (dofs, h) {
                (Some(n), _) => Resolution::Dofs(n),
                (None, Some(h)) => Resolution::Size(h),
                (None, None) => Resolution::Dofs(150_000),
            };
            match analyse(&doc, res, question::clock) {
                Ok(a) => {
                    let fieldsmith_core::Analysis {
                        provenance, cases, ..
                    } = a;
                    ok_reply(schema::Response {
                        apiv: APIV,
                        engine: store::engine_identity(),
                        status: Status::Ok,
                        name: Some(name),
                        question: Some(echo),
                        warnings,
                        outcome: None,
                        single: Some(schema::SingleResult {
                            provenance,
                            cases,
                            modes: None,
                        }),
                        sweep: None,
                        error: None,
                    })
                }
                Err(SetupError(e)) => error_reply(APIV, Some(&name), Some(&echo), warnings, e),
            }
        }
        QuestionSpec::Modes { n, dofs, h } => {
            if doc.fill.is_some() {
                return error_reply(
                    APIV,
                    Some(&name),
                    Some(&echo),
                    warnings,
                    "modal runs the solid path — remove the fill statement",
                );
            }
            if n == 0 {
                return error_reply(
                    APIV,
                    Some(&name),
                    Some(&echo),
                    warnings,
                    "modes: n must be at least 1",
                );
            }
            if !doc.loads.is_empty() {
                // Same statement the CLI prints: a modal run needs the part held, not
                // pushed — the loads are dropped, not refused.
                warnings.push("loads ignored: a modal run needs the part held, not pushed".into());
            }
            let res = match (dofs, h) {
                (Some(n), _) => Resolution::Dofs(n),
                (None, Some(h)) => Resolution::Size(h),
                (None, None) => Resolution::Dofs(150_000),
            };
            match analyse_modes(&doc, res, question::clock, n) {
                Ok(run) => {
                    let n_req = n;
                    ok_reply(schema::Response {
                        apiv: APIV,
                        engine: store::engine_identity(),
                        status: Status::Ok,
                        name: Some(name),
                        question: Some(echo),
                        warnings,
                        outcome: None,
                        single: Some(schema::SingleResult {
                            provenance: run.provenance,
                            cases: Vec::new(),
                            modes: Some(schema::ModesJson {
                                n_requested: n_req,
                                frequencies_hz: run.modal.frequencies,
                            }),
                        }),
                        sweep: None,
                        error: None,
                    })
                }
                Err(SetupError(e)) => error_reply(APIV, Some(&name), Some(&echo), warnings, e),
            }
        }
        QuestionSpec::Sweep {
            param,
            values,
            rungs,
            tolerance,
        } => {
            if doc.fill.is_some() {
                return error_reply(
                    APIV,
                    Some(&name),
                    Some(&echo),
                    warnings,
                    "the sweep runs the solid path — remove the fill statement",
                );
            }
            // The address must name a numeric parameter of THIS document before any
            // point runs: unknown id or non-numeric attr is a request error naming
            // what failed (param::resolve's text lists what the statement takes).
            let addr = match param::ParamAddr::parse(&param) {
                Ok(a) => a,
                Err(e) => return error_reply(APIV, Some(&name), Some(&echo), warnings, e),
            };
            if let Err(e) = param::resolve(&doc, &addr) {
                return error_reply(APIV, Some(&name), Some(&echo), warnings, e);
            }
            if values.is_empty() {
                return error_reply(
                    APIV,
                    Some(&name),
                    Some(&echo),
                    warnings,
                    "sweep: values is empty — name at least one value of the parameter",
                );
            }
            if values.iter().any(|v| !v.is_finite()) {
                return error_reply(
                    APIV,
                    Some(&name),
                    Some(&echo),
                    warnings,
                    "sweep: values must all be finite numbers",
                );
            }
            let rungs = match rungs {
                None => question::DEFAULT_RUNGS.to_vec(),
                Some(v) => match question::validate_rungs(&v) {
                    Ok(()) => v,
                    Err(e) => return error_reply(APIV, Some(&name), Some(&echo), warnings, e),
                },
            };
            let tolerance = match tolerance {
                None => None,
                Some(s) => match Tolerance::parse(&s) {
                    Ok(t) => Some((t, s.clone())),
                    Err(e) => {
                        return error_reply(APIV, Some(&name), Some(&echo), warnings, e);
                    }
                },
            };
            // One machine probe, one store read: every point seeds from the same
            // picture; only the document copy and its applied value change.
            let machine = store::Machine::probe();
            let store_rows = store::store_path()
                .map(|p| store::read_all(&p))
                .unwrap_or_default();
            let base = question::Question {
                rungs,
                tolerance,
                // A per-point budget is outside the sweep grammar: each point runs
                // to its tolerance or the schedule's cap.
                budget_ms: None,
                store_rows,
                machine,
                clock: question::clock,
            };
            let mut points = Vec::with_capacity(values.len());
            for (i, value) in values.iter().enumerate() {
                let mut pd = doc.clone();
                // apply re-runs resolve, which accepted this document above; the
                // error arm cannot fire, but the honest path names the point if it
                // ever did.
                if let Err(e) = param::apply(&mut pd, &addr, *value) {
                    return error_reply(
                        APIV,
                        Some(&name),
                        Some(&echo),
                        warnings,
                        format!("sweep point {} (param {param} = {value}): {e}", i + 1),
                    );
                }
                let q = base.clone();
                match question::run(&pd, &q, &mut Collect) {
                    Ok(o) => points.push(schema::SweepPoint {
                        value: *value,
                        outcome: outcome_json(&pd, o),
                    }),
                    Err(SetupError(e)) => {
                        return error_reply(
                            APIV,
                            Some(&name),
                            Some(&echo),
                            warnings,
                            format!(
                                "sweep point {} of {} (param {param} = {value}): {e}",
                                i + 1,
                                values.len()
                            ),
                        );
                    }
                }
            }
            // The sweep always answers ok — each point carries its own verdict; a
            // declared per-point tolerance that a point missed stays inside that
            // point's outcome (its refusal text) rather than failing the sweep.
            ok_reply(schema::Response {
                apiv: APIV,
                engine: store::engine_identity(),
                status: Status::Ok,
                name: Some(name),
                question: Some(echo),
                warnings,
                outcome: None,
                single: None,
                sweep: Some(schema::SweepJson {
                    param,
                    values,
                    points,
                }),
                error: None,
            })
        }
    }
}

/// The ladder answer's envelope form — the ladder arm and each sweep point build the
/// same JSON through this one function (one composition, no second presentation to
/// drift).
fn outcome_json(doc: &Document, o: question::Outcome) -> schema::OutcomeJson {
    let refusal = o.unmet.as_ref().map(question::refusal_text);
    schema::OutcomeJson {
        records: o.records,
        total_ms: o.total_ms,
        field_leaves: expr::active_leaves(doc).len(),
        cost_fit: o.fit.map(|f| schema::FitJson {
            exponent: f.exponent,
            ms_at_unit: f.ms_at_unit,
            residual_rel: f.residual,
        }),
        projections: o.projections,
        notes: o.notes,
        stop_reason: o.stop_reason,
        met: o.met,
        unmet: o.unmet,
        refusal,
    }
}

fn error_reply(
    apiv: u32,
    name: Option<&str>,
    question: Option<&QuestionSpec>,
    warnings: Vec<String>,
    message: impl Into<String>,
) -> Reply {
    final_reply(
        Response {
            apiv,
            engine: store::engine_identity(),
            status: Status::Error,
            name: name.map(str::to_string),
            question: question.cloned(),
            warnings,
            outcome: None,
            single: None,
            sweep: None,
            error: Some(message.into()),
        },
        true,
    )
}

fn ok_reply(r: Response) -> Reply {
    final_reply(r, false)
}

fn final_reply(r: Response, is_error: bool) -> Reply {
    // Compact one line (the envelope is a contract, not a display). The typed
    // response rides along for the CSV builder — see Reply::response.
    let body = serde_json::to_string(&r).unwrap_or_else(|e| {
        // A Response of Strings and floats cannot fail to serialize; the escape
        // hatch stays for honesty about the path, not as a live branch.
        format!("{{\"apiv\":{APIV},\"engine\":\"\",\"status\":\"error\",\"name\":null,\"question\":null,\"warnings\":[],\"error\":\"serialize: {e}\"}}")
    });
    Reply {
        body,
        is_error,
        response: Some(r),
    }
}

/// The API's [`question::Sink`]: records land in the Outcome; nothing is streamed
/// anywhere (one answer, once the question is done).
struct Collect;

impl question::Sink for Collect {
    fn record(&mut self, _rec: &fieldsmith_core::rung::RungRecord) {}
    fn note(&mut self, _text: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "\
fieldsmith 1
name \"sweep test\"
custommaterial steel200 name=\"Steel\" e=200000 nu=0.3 yield=250 density=7.85
material steel200
box beam size=(100, 10, 10) at=(50, 5, 5)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -100)";

    fn sweep_request(param: &str, values: &str, extra: &str) -> String {
        format!(
            "{{\"apiv\": 2, \"doc\": {DOC:?}, \"question\": {{\"kind\": \"sweep\", \
             \"param\": {param:?}, \"values\": {values}{extra}}}}}"
        )
    }

    /// The V1 cantilever at a small ladder keeps the sweep unit tests in the
    /// milliseconds range (referee: two rungs of a 2k/4k ladder solve in single-digit
    /// ms; the measured boxes are V23's business, not these tests').
    const FAST_RUNGS: &str = ", \"rungs\": [2000, 4000]";

    fn envelope(input: &str) -> serde_json::Value {
        let r = respond(input);
        serde_json::from_str(&r.body).expect("envelope parses")
    }

    #[test]
    fn unknown_param_is_a_named_request_error() {
        let e = envelope(&sweep_request("nothere.size.0", "[1]", FAST_RUNGS));
        assert_eq!(e["status"], "error");
        assert!(e["error"]
            .as_str()
            .expect("error text")
            .contains("no node, load or temperature named \"nothere\""));
    }

    #[test]
    fn non_numeric_attr_names_what_the_statement_takes() {
        let e = envelope(&sweep_request("beam.major", "[1]", FAST_RUNGS));
        assert_eq!(e["status"], "error");
        let text = e["error"].as_str().expect("error text");
        assert!(text.contains("has no numeric parameter \"major\""));
        assert!(
            text.contains("size[0-2]"),
            "the note names what a box takes"
        );
        // The scalar rule: index on a scalar, no index on a vector, both named.
        let e2 = envelope(&sweep_request("beam.round.1", "[1]", FAST_RUNGS));
        assert!(e2["error"]
            .as_str()
            .unwrap()
            .contains("takes no component index"));
        let e3 = envelope(&sweep_request("beam.size", "[1]", FAST_RUNGS));
        assert!(e3["error"]
            .as_str()
            .unwrap()
            .contains("needs a component index"));
        let e4 = envelope(&sweep_request("beam.size.3", "[1]", FAST_RUNGS));
        assert!(e4["error"].as_str().unwrap().contains("out of range"));
    }

    #[test]
    fn empty_and_bad_values_are_request_errors() {
        let e = envelope(&sweep_request("beam.size.2", "[]", FAST_RUNGS));
        assert!(e["error"].as_str().unwrap().contains("values is empty"));
        // A declared tolerance that does not parse is refused before any point runs.
        let e2 = envelope(&sweep_request(
            "beam.size.2",
            "[1]",
            ", \"tolerance\": \"deflection: 15 apples\"",
        ));
        assert!(e2["error"].as_str().unwrap().contains("tolerance"));
        // One-rung schedules are refused the same way the ladder question refuses them.
        let e3 = envelope(&sweep_request("beam.size.2", "[1]", ", \"rungs\": [4000]"));
        assert!(e3["error"].as_str().unwrap().contains("at least two rungs"));
    }

    #[test]
    fn apiv_mismatch_names_the_sweep_change() {
        let req = format!(
            "{{\"apiv\": 1, \"doc\": {DOC:?}, \"question\": {{\"kind\": \"sweep\", \
             \"param\": \"beam.size.2\", \"values\": [1]}}}}"
        );
        let e = envelope(&req);
        assert_eq!(e["status"], "error");
        assert_eq!(e["apiv"], 1);
        let text = e["error"].as_str().expect("error text");
        assert!(text.contains("apiv 2 added the sweep question"), "{text}");
        assert!(text.contains("speaks 2"));
    }

    #[test]
    fn one_point_per_value_in_order_with_last_rung_numbers() {
        let body = envelope(&sweep_request("beam.size.2", "[6, 10]", FAST_RUNGS));
        assert_eq!(body["status"], "ok");
        assert_eq!(body["question"]["kind"], "sweep");
        assert_eq!(body["sweep"]["param"], "beam.size.2");
        let pts = body["sweep"]["points"].as_array().expect("points");
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[0]["value"], 6.0);
        assert_eq!(pts[1]["value"], 10.0);
        // Each point ran its own ladder: two rungs each, the change columns present.
        for p in pts {
            let out = &p["outcome"];
            assert_eq!(out["records"].as_array().unwrap().len(), 2);
            assert!(out["records"][1]["tracked"]["deflection_mm"].is_number());
            assert!(out["total_ms"].as_f64().unwrap() > 0.0);
            // The met/unmet shape: no tolerance declared, neither key present.
            assert!(out["met"].is_null() && out["unmet"].is_null());
        }
        // Thinner Z is much softer — the ordered sweep must disagree across its points
        // (δ ∝ 1/h³ is V23's measured business; here one inequality only).
        let d0 = pts[0]["outcome"]["records"][1]["tracked"]["deflection_mm"]
            .as_f64()
            .unwrap();
        let d1 = pts[1]["outcome"]["records"][1]["tracked"]["deflection_mm"]
            .as_f64()
            .unwrap();
        assert!(d0 > d1, "h=6 deflection {d0} must exceed h=10 {d1}");
        // The sweep envelope answers ok even with nothing declared at the sweep level.
    }

    #[test]
    fn declared_tolerance_lands_inside_each_point() {
        let body = envelope(&sweep_request(
            "beam.size.2",
            "[6, 10]",
            ", \"tolerance\": \"deflection:0.1%\"",
        ));
        let pts = body["sweep"]["points"].as_array().unwrap();
        for p in pts {
            let out = &p["outcome"];
            let met = out["met"].as_object();
            let unmet = out["unmet"].as_object();
            assert!(
                met.is_some() || unmet.is_some(),
                "each point carries a verdict"
            );
            if unmet.is_some() {
                assert!(out["refusal"].as_str().unwrap().contains("declared"));
            }
        }
        // The sweep envelope itself stays ok regardless of the per-point verdicts.
        assert_eq!(body["status"], "ok");
    }

    const LET_DOC: &str = "fieldsmith 1
name \"sweep test\"
custommaterial steel200 name=\"Steel\" e=200000 nu=0.3 yield=250 density=7.85
material steel200
let w = 10
box beam size=(100, 10, w) at=(50, 5, 5)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -100)";

    fn sweep_request_doc(doc: &str, param: &str, values: &str, extra: &str) -> String {
        format!(
            "{{\"apiv\": 2, \"doc\": {doc:?}, \"question\": {{\"kind\": \"sweep\", \
             \"param\": {param:?}, \"values\": {values}{extra}}}}}"
        )
    }

    /// Wall-clock columns differ between two runs of the same physics; drop them and
    /// the cost fit they feed everywhere below the outcome so the comparison stays
    /// physical.
    fn strip_wall_ms(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("wall_ms");
                map.remove("total_ms");
                map.remove("cost_fit");
                map.values_mut().for_each(strip_wall_ms);
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(strip_wall_ms),
            _ => {}
        }
    }

    #[test]
    fn let_sweep_matches_the_node_path_sweep() {
        // With w = 10 the let document is the same cantilever; sweeping the let and
        // sweeping the node path it drives must produce the same numbers.
        let body = envelope(&sweep_request_doc(LET_DOC, "w", "[6, 10]", FAST_RUNGS));
        let plain = envelope(&sweep_request("beam.size.2", "[6, 10]", FAST_RUNGS));
        assert_eq!(body["status"], "ok", "let sweep runs");
        assert_eq!(body["sweep"]["param"], "w");
        let pts = body["sweep"]["points"].as_array().expect("points");
        let want = plain["sweep"]["points"].as_array().expect("plain points");
        assert_eq!(pts.len(), want.len());
        for (p, q) in pts.iter().zip(want.iter()) {
            assert_eq!(p["value"], q["value"]);
            let mut a = p["outcome"].clone();
            let mut b = q["outcome"].clone();
            strip_wall_ms(&mut a);
            strip_wall_ms(&mut b);
            assert_eq!(a, b, "outcomes minus wall-clock must match");
        }
    }

    #[test]
    fn sweeping_a_driven_attr_refuses_with_the_let_hint() {
        let e = envelope(&sweep_request_doc(
            LET_DOC,
            "beam.size.2",
            "[6, 10]",
            FAST_RUNGS,
        ));
        assert_eq!(e["status"], "error");
        let text = e["error"].as_str().expect("error text");
        assert!(text.contains("written by an expression"), "{text}");
        assert!(text.contains("sweep the let that drives it"), "{text}");
    }
}
