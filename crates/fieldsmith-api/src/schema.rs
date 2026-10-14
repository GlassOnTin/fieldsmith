//! The versioned request/response envelope (GOAL v0.4 section C): one JSON request in,
//! one JSON envelope out. `apiv` 1. The request is strict — unknown top-level keys are
//! refused (`deny_unknown_fields`); the question enum is internally tagged, which serde
//! cannot make strict, so unknown keys INSIDE a question are silently ignored — a named
//! serde limitation, stated here and in the V22 notes, so the gap is contract rather
//! than accident.

use fieldsmith_core::rung::RungRecord;
use fieldsmith_core::{CaseResult, Provenance};

use crate::question::{Met, Projection, Unmet};
use serde::{Deserialize, Serialize};

/// The envelope contract version. Bumping it is a named change — v2 (2026-10-14, M16)
/// adds the `sweep` question: `{"kind": "sweep", "param": "<id>.<attr>[.<n>]",
/// "values": [...], …}` with the per-point rungs/tolerance defaults. v1 requests are
/// refused with a line naming the change; see `respond`.
pub const APIV: u32 = 2;

// ---------------------------------------------------------------- request ----

/// One API request: the document and one question about it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Envelope version the request speaks; must equal [`APIV`].
    pub apiv: u32,
    /// .fsm source text. Exactly one of `doc` / `docz` must be present; a `doc`
    /// carries no mesh grids, so a mesh document's grids must already be in the
    /// calling process's registry (or the request is refused — see [`crate::respond`]).
    #[serde(default)]
    pub doc: Option<String>,
    /// base64 `.fsmz` bytes (grids included; canonical padding required, whitespace
    /// between groups tolerated).
    #[serde(default)]
    pub docz: Option<String>,
    pub question: QuestionSpec,
}

/// The question about the document — the grammar the CLI's solver flags declare.
/// Exact one-of-a-kind: a request can ask one question.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuestionSpec {
    /// The declared-question ladder. `rungs` default [`crate::question::DEFAULT_RUNGS`],
    /// `tolerance` the CLI's `--tolerance` string form, `budget` the CLI's `--budget`
    /// form (`30s`, `2m`).
    Ladder {
        #[serde(default)]
        rungs: Option<Vec<usize>>,
        #[serde(default)]
        tolerance: Option<String>,
        #[serde(default)]
        budget: Option<String>,
    },
    /// One static solve at the stated resolution (`dofs` target, or explicit `h` mm;
    /// default like the CLI: 150_000 DOF).
    Solve {
        #[serde(default)]
        dofs: Option<usize>,
        #[serde(default)]
        h: Option<f64>,
    },
    /// `n` natural frequencies at the stated resolution (no load solving: loads are
    /// ignored the same way the CLI says, supports still apply).
    Modes {
        n: usize,
        #[serde(default)]
        dofs: Option<usize>,
        #[serde(default)]
        h: Option<f64>,
    },
    /// The sweep (M16): one named numeric document parameter, one stated list of values.
    /// Each value is applied to a copy of the document and asks the same ladder question
    /// at it — `rungs` / `tolerance` default like [`Ladder`] and apply to EVERY point;
    /// a per-point budget is outside this grammar (each point runs to its tolerance or
    /// a shared schedule's cap). The param must resolve against the parsed document
    /// (unknown id, non-numeric attr: request error naming what failed, see
    /// `fieldsmith_doc::param`).
    Sweep {
        param: String,
        values: Vec<f64>,
        #[serde(default)]
        rungs: Option<Vec<usize>>,
        #[serde(default)]
        tolerance: Option<String>,
    },
}

// --------------------------------------------------------------- response ----

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The question was answered (including a declared-tolerance met answer).
    Ok,
    /// The tolerance was declared and the rungs did not meet it — a refusal is an
    /// honest answer, not a failure (exit 0 at the one-shot; 3 at the CLI).
    Refusal,
    /// Request error (bad request, setup error): `error` names it.
    Error,
}

/// One API envelope.
#[derive(Debug, Serialize)]
pub struct Response {
    /// The envelope version this response speaks (always [`APIV`]).
    pub apiv: u32,
    /// The engine that answered (rows/reporting pin it; see store::engine_identity).
    pub engine: String,
    pub status: Status,
    /// The document's name, once the request parsed far enough to load it.
    pub name: Option<String>,
    /// Echo of the request's question (every envelope whose request at least parsed).
    pub question: Option<QuestionSpec>,
    /// Document material warnings (the CLI prints them on stderr).
    pub warnings: Vec<String>,
    /// Present for the ladder question.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<OutcomeJson>,
    /// Present for the solve / modes questions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub single: Option<SingleResult>,
    /// Present for the sweep question: one point per stated value, in order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sweep: Option<SweepJson>,
    /// Named error for `status: error`.
    pub error: Option<String>,
}

/// The ladder answer: records, totals, costs, notes, and the met/refusal components.
#[derive(Debug, Serialize)]
pub struct OutcomeJson {
    /// One record per rung actually bought, in schedule order.
    pub records: Vec<RungRecord>,
    /// Sum of the rungs' host-measured wall times, ms.
    pub total_ms: f64,
    /// Active field leaves of the document (the store row's complexity proxy).
    pub field_leaves: usize,
    /// Cost fit over the rungs actually run (≥2 rungs); the CLI's prose prints the
    /// same numbers.
    pub cost_fit: Option<FitJson>,
    /// The schedule entries the answer did not buy, with their projections.
    pub projections: Vec<Projection>,
    /// Notes made while the ladder ran (e.g. a buyable rung with no cost estimate).
    pub notes: Vec<String>,
    /// The early-stop reason when the budget cut the ladder.
    pub stop_reason: Option<String>,
    /// Tolerance met: which rung and the change that met it.
    pub met: Option<Met>,
    /// Tolerance declared and unmet: the refusal's components.
    pub unmet: Option<Unmet>,
    /// The refusal sentence (the CLI prints "refusal: …" with the same text — one
    /// composition, two presentations).
    pub refusal: Option<String>,
}

/// The cost fit's envelope form (CostFit fields, `residual` renamed to make the
/// relative meaning explicit; serde_json never sees the struct itself).
#[derive(Debug, Serialize)]
pub struct FitJson {
    /// Slope in log–log space (wall ∝ DOF^exponent).
    pub exponent: f64,
    /// Wall time (ms) at 1 DOF — the fit's scale.
    pub ms_at_unit: f64,
    /// Max relative deviation over the fitted points; `null` with exactly two rungs
    /// (the fit is exact by construction, unmeasured).
    pub residual_rel: Option<f64>,
}

/// A one-rung answer: the provenance, the case results (thermal cases carry their
/// nodal temperature array), and the modes question's frequencies.
#[derive(Debug, Serialize)]
pub struct SingleResult {
    pub provenance: Provenance,
    pub cases: Vec<CaseResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modes: Option<ModesJson>,
}

/// The modes question's answer.
#[derive(Debug, Serialize)]
pub struct ModesJson {
    /// The n the request asked for (the solver may return fewer on degenerate docs).
    pub n_requested: usize,
    /// Natural frequencies, Hz, in order.
    pub frequencies_hz: Vec<f64>,
}

/// The sweep question's answer: the addressed parameter, the stated values, and one
/// point per value in order (each point's rungs, boxes, costs and tolerance verdict as
/// its own [`OutcomeJson`]). `total_ms` is the sum of the per-point rung walls (the
/// host's own envelope wall around all points is not part of the honest contract —
/// every other envelope names billed rung time, not caller overhead).
#[derive(Debug, Serialize)]
pub struct SweepJson {
    /// The parameter address exactly as the request stated it.
    pub param: String,
    /// The values, in request order (the points are in the same order).
    pub values: Vec<f64>,
    /// One answer per value, in request order.
    pub points: Vec<SweepPoint>,
}

/// One sweep point: the value applied to the document copy, and that point's own
/// declared-question outcome (its last rung's tracked values are what the CSV takes).
#[derive(Debug, Serialize)]
pub struct SweepPoint {
    pub value: f64,
    pub outcome: OutcomeJson,
}
