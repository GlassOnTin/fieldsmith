//! The declared-question runner (GOAL v0.4 sections A and C): the ONE verified loop
//! that walks the rung schedule, records each rung, and answers — stop inside the
//! declared tolerance, spend up to the declared budget, or leave the named refusal for
//! the caller to present. The CLI streams its prose through a [`Sink`] implementation;
//! the JSON API collects the same events into its envelope. Every rung's wall time is
//! one host measurement around mesh+solve; the record fields are the engine's own
//! numbers; the store seeds the estimator before the run has its own fit and the runner
//! never writes it — writing stays with the CLI (and later the worker host).

use fieldsmith_core::rung::{CostFit, RungChange, RungRecord, Tolerance};
use fieldsmith_core::{analyse, Clock, Resolution, SetupError};
use fieldsmith_doc::Document;
use serde::Serialize;

use crate::store::{seed_median, Machine, Row};

/// The rung schedule the declared-question modes walk by default. The first two entries
/// match the web's coarse/refined constants; the third extends the ladder for V17.
pub const DEFAULT_RUNGS: [usize; 3] = [30_000, 150_000, 750_000];

/// The process-relative millisecond clock the engine's `Provenance` timings are built
/// against (the CLI's is the same shape — process uptime matters, not a wall hour).
pub fn clock() -> f64 {
    thread_local!(static T0: std::time::Instant = std::time::Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

/// `30s`, `2m` — seconds or minutes, returned as seconds (the caller converts to ms;
/// the API budget is stated the way a person states it). The error is neutral — callers
/// wrap it with their own presentation (the CLI adds the flag name, the API the field).
pub fn parse_budget(s: &str) -> Result<f64, String> {
    let bad = || "expect e.g. 30s or 2m".to_string();
    let (num, unit) = s.split_at(s.len().saturating_sub(1));
    let v: f64 = num.trim().parse().map_err(|_| bad())?;
    match unit {
        "s" => Ok(v),
        "m" => Ok(v * 60.0),
        _ => Err(bad()),
    }
}

/// The schedule's structural rule, validated before anything runs: at least two rungs
/// (the change estimate needs a baseline), strictly increasing.
pub fn validate_rungs(rungs: &[usize]) -> Result<(), String> {
    if rungs.len() < 2 {
        return Err(
            "rungs: a ladder needs at least two rungs — the rung-to-rung change \
                    estimate needs a baseline"
                .into(),
        );
    }
    if !rungs.windows(2).all(|w| w[0] < w[1]) {
        return Err(
            "rungs: rung DOF targets must increase (the change estimate reads \
                    refinement, not coarsening)"
                .into(),
        );
    }
    Ok(())
}

/// The parsed, validated question; the schema layer's strings land here parsed.
#[derive(Clone)]
pub struct Question {
    pub rungs: Vec<usize>,
    /// The parsed tolerance with the raw string it was written as — the met/refusal
    /// answers quote the user's own words back.
    pub tolerance: Option<(Tolerance, String)>,
    /// Budget in milliseconds.
    pub budget_ms: Option<f64>,
    /// Store rows that seed the estimator before the run has its own fit (read-only).
    pub store_rows: Vec<Row>,
    pub machine: Machine,
    pub clock: Clock,
}

/// Where the runner's events go while it works. The CLI prints prose; the JSON layer
/// accumulates. Records arrive in schedule order as each rung finishes.
pub trait Sink {
    fn record(&mut self, rec: &RungRecord);
    fn note(&mut self, text: &str);
}

/// A rung's projected wall time and where it came from — a fit over the rungs run so
/// far, or the median of stored local runs on the same machine class.
#[derive(Clone, Debug, Serialize)]
pub struct Estimate {
    pub ms: f64,
    /// "fit" or "seed-median".
    pub source: &'static str,
    /// The provenance clause the prose quotes.
    pub detail: String,
}

/// A rung's wall-time projection for the schedule entries the answer did not buy.
#[derive(Clone, Debug, Serialize)]
pub struct Projection {
    pub target_dofs: usize,
    /// Fit projection over the rungs that ran; absent without a fit (≥2 rungs needed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fit_ms: Option<f64>,
    /// Median of the stored local runs on the same machine class, engine and schedule
    /// entry; absent with no matching records.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed_n: Option<usize>,
}

/// A tolerance that the ladder met, with the numbers that met it.
#[derive(Clone, Debug, Serialize)]
pub struct Met {
    /// The tolerance as the user wrote it.
    pub tolerance: String,
    pub change: RungChange,
    pub rung: usize,
    pub dofs: usize,
    pub rungs_run: usize,
}

/// A declared tolerance the rungs did not meet — the components of the named refusal.
#[derive(Clone, Debug, Serialize)]
pub struct Unmet {
    /// The tolerance as the user wrote it.
    pub tolerance: String,
    /// The last rung's change; absent when the ladder stopped before a second rung.
    pub change: Option<RungChange>,
    /// The early-stop reason when the budget cut the ladder; absent when the schedule
    /// ran to its cap.
    pub stop_reason: Option<String>,
    /// The schedule's cap (its last DOF target).
    pub cap_dofs: usize,
}

/// What the ladder produced — records plus every number both the CLI prose and the JSON
/// envelope present.
#[derive(Debug)]
pub struct Outcome {
    pub records: Vec<RungRecord>,
    pub total_ms: f64,
    /// Cost fit over the rungs actually run (≥2 rungs with distinct positive DOF).
    pub fit: Option<CostFit>,
    /// Set when the budget stopped the ladder early — the reason text.
    pub stop_reason: Option<String>,
    /// Notes made while the ladder ran (budget-estimate gaps, ...).
    pub notes: Vec<String>,
    pub met: Option<Met>,
    /// Set when a tolerance was declared and unmet.
    pub unmet: Option<Unmet>,
    pub projections: Vec<Projection>,
}

/// Affordability of the next rung: the fit over the rungs so far when one exists,
/// else the stored history from the same machine class.
pub fn estimate_for(target: usize, records: &[RungRecord], q: &Question) -> Option<Estimate> {
    let pts: Vec<(f64, f64)> = records.iter().map(|r| (r.dofs as f64, r.wall_ms)).collect();
    if let Some(f) = CostFit::of(&pts) {
        let res = match f.residual {
            Some(r) => format!("fit residual {:.0} %", 100.0 * r),
            None => "two-rung fit, residual unmeasured".into(),
        };
        return Some(Estimate {
            ms: f.project(target as f64),
            source: "fit",
            detail: format!("{} rungs so far, {res}", pts.len()),
        });
    }
    seed_median(&q.store_rows, &q.machine, target).map(|(med, n)| Estimate {
        ms: med,
        source: "seed-median",
        detail: format!("median of {n} local record(s)"),
    })
}

/// Run the declared question against `doc`, streaming rung records and notes through
/// `sink`. A setup error (bad material, unmeetable geometry, missing grids) is the
/// caller's error, not an answer.
pub fn run(doc: &Document, q: &Question, sink: &mut dyn Sink) -> Result<Outcome, SetupError> {
    let mut records: Vec<RungRecord> = Vec::new();
    let mut stop_reason: Option<String> = None;
    let mut met: Option<Met> = None;
    let mut notes: Vec<String> = Vec::new();

    for (i, &target) in q.rungs.iter().enumerate() {
        // Affordability of rung i is checked before buying it (rung 0 always runs); the
        // tolerance check sits after the rung whose change it reads.
        if i > 0 {
            if let Some(b) = q.budget_ms {
                let spent: f64 = records.iter().map(|r| r.wall_ms).sum();
                match estimate_for(target, &records, q) {
                    Some(e) => {
                        if spent + e.ms > b {
                            stop_reason = Some(format!(
                                "budget: {:.1} s of the {:.1} s spent, rung {i} ({}k DOF) \
                                 projected at about {:.1} s ({}) does not fit — stopping",
                                spent / 1000.0,
                                b / 1000.0,
                                target / 1000,
                                e.ms / 1000.0,
                                e.detail,
                            ));
                            break;
                        }
                    }
                    None => {
                        let note = format!(
                            "budget: no estimate for rung {i} — the fit needs two rungs and \
                             no matching local record exists; buying the rung and measuring"
                        );
                        sink.note(&note);
                        notes.push(note);
                    }
                }
            }
        }
        let t0 = std::time::Instant::now();
        let a = analyse(doc, Resolution::Dofs(target), q.clock)?;
        let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let rec = RungRecord::of(i, target, wall_ms, records.last(), &a);
        sink.record(&rec);
        records.push(rec);
        // The change this rung just measured is what the tolerance is declared against.
        if i > 0 && met.is_none() {
            let tol = q.tolerance.as_ref().map(|(t, _)| *t);
            if let (Some(t), Some(c)) = (tol, records[i].change) {
                if t.met(&c) {
                    met = Some(Met {
                        rung: i,
                        dofs: records[i].dofs,
                        rungs_run: records.len(),
                        change: c,
                        tolerance: q
                            .tolerance
                            .as_ref()
                            .map(|(_, raw)| raw.clone())
                            .unwrap_or_default(),
                    });
                    break;
                }
            }
        }
    }

    let total_ms: f64 = records.iter().map(|r| r.wall_ms).sum();

    // Cost fit over the rungs actually run, then projections and stored history for
    // the rungs the answer did not buy.
    let pts: Vec<(f64, f64)> = records.iter().map(|r| (r.dofs as f64, r.wall_ms)).collect();
    let fit = CostFit::of(&pts);
    let mut projections = Vec::new();
    for &target in q.rungs.iter().skip(records.len()) {
        let seed = seed_median(&q.store_rows, &q.machine, target);
        projections.push(Projection {
            target_dofs: target,
            fit_ms: fit.as_ref().map(|f| f.project(target as f64)),
            seed_ms: seed.as_ref().map(|(ms, _)| *ms),
            seed_n: seed.as_ref().map(|(_, n)| *n),
        });
    }

    let unmet = if q.tolerance.is_some() && met.is_none() {
        Some(Unmet {
            tolerance: q
                .tolerance
                .as_ref()
                .map(|(_, raw)| raw.clone())
                .unwrap_or_default(),
            change: records.last().and_then(|r| r.change),
            stop_reason: stop_reason.clone(),
            cap_dofs: q.rungs.last().copied().unwrap_or(0),
        })
    } else {
        None
    };

    Ok(Outcome {
        records,
        total_ms,
        fit,
        stop_reason,
        notes,
        met,
        unmet,
        projections,
    })
}

/// The named refusal's full sentence, shared by the CLI ("refusal: …") and the JSON
/// envelope's `refusal` field — one composition, two presentations.
pub fn refusal_text(u: &Unmet) -> String {
    let why = match &u.stop_reason {
        Some(r) => format!("{r};"),
        None => format!("the schedule ran to its cap ({} DOF);", u.cap_dofs),
    };
    match &u.change {
        Some(c) => format!(
            "{why} the change still sits outside {}: deflection {:+.1} % ({:+.4} mm), \
             von Mises {:+.1} % ({:+.2} MPa).",
            u.tolerance,
            100.0 * c.deflection_rel,
            c.deflection_mm,
            100.0 * c.vm_rel,
            c.vm_mpa,
        ),
        None => format!(
            "{why} the ladder stopped before a second rung, so no rung-to-rung change \
             exists and {} is unmet.",
            u.tolerance
        ),
    }
}

/// The "met" answer's sentence after its "met: " prefix — the CLI prints it; the JSON
/// envelope carries the structured [`Met`].
pub fn met_text(m: &Met, total_ms: f64) -> String {
    format!(
        "deflection {:+.1} % ({:+.4} mm), von Mises {:+.1} % ({:+.2} MPa) sits inside {} \
         — rung {} ({} DOF), total {:.1} s over {} rungs.",
        100.0 * m.change.deflection_rel,
        m.change.deflection_mm,
        100.0 * m.change.vm_rel,
        m.change.vm_mpa,
        m.tolerance,
        m.rung,
        m.dofs,
        total_ms / 1000.0,
        m.rungs_run,
    )
}
