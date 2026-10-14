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

use fieldsmith_core::rung::Tolerance;
use fieldsmith_core::{analyse, analyse_modes, fsmz, Resolution, SetupError};
use fieldsmith_doc::{fsm, Document, NodeKind};
use fieldsmith_field::expr;
use fieldsmith_field::meshfield::{key_from_hex, lookup};

use schema::{QuestionSpec, Request, Response, Status, APIV};

/// One answer: the compact JSON on one line (no trailing newline — stdout adds one)
/// plus whether it is a processing error (`status: error`), which the one-shot maps to
/// exit code 2. Ok and refusal are both `is_error: false`.
pub struct Reply {
    pub body: String,
    pub is_error: bool,
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
            fsmz::load(&bytes).map(|l| l.doc).map_err(|e| format!("docz: {e}"))
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

    // 2. Version.
    if req.apiv != APIV {
        return error_reply(
            req.apiv,
            None,
            Some(&req.question),
            Vec::new(),
            format!(
                "apiv: request declares {}, this engine speaks {APIV}",
                req.apiv
            ),
        );
    }

    // 3–4. Load the document (the shared loader the document-openers use too).
    let doc: Document = match load_doc(req.doc.as_deref(), req.docz.as_deref()) {
        Ok(d) => d,
        Err(message) => {
            return error_reply(
                APIV,
                None,
                Some(&req.question),
                Vec::new(),
                message,
            )
        }
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
                    let refusal = o.unmet.as_ref().map(question::refusal_text);
                    let outcome = schema::OutcomeJson {
                        records: o.records,
                        total_ms: o.total_ms,
                        field_leaves: expr::active_leaves(&doc).len(),
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
                    };
                    ok_reply(schema::Response {
                        apiv: APIV,
                        engine: store::engine_identity(),
                        status,
                        name: Some(name),
                        question: Some(echo),
                        warnings,
                        outcome: Some(outcome),
                        single: None,
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
                        error: None,
                    })
                }
                Err(SetupError(e)) => error_reply(APIV, Some(&name), Some(&echo), warnings, e),
            }
        }
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
            error: Some(message.into()),
        },
        true,
    )
}

fn ok_reply(r: Response) -> Reply {
    final_reply(r, false)
}

fn final_reply(r: Response, is_error: bool) -> Reply {
    Reply {
        // Compact one line (the envelope is a contract, not a display).
        body: serde_json::to_string(&r).unwrap_or_else(|e| {
            // A Response of Strings and floats cannot fail to serialize; the escape
            // hatch stays for honesty about the path, not as a live branch.
            format!("{{\"apiv\":{APIV},\"engine\":\"\",\"status\":\"error\",\"name\":null,\"question\":null,\"warnings\":[],\"error\":\"serialize: {e}\"}}")
        }),
        is_error,
    }
}

/// The API's [`question::Sink`]: records land in the Outcome; nothing is streamed
/// anywhere (one answer, once the question is done).
struct Collect;

impl question::Sink for Collect {
    fn record(&mut self, _rec: &fieldsmith_core::rung::RungRecord) {}
    fn note(&mut self, _text: &str) {}
}
