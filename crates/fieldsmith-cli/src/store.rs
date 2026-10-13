//! The local rung-cost store (GOAL v0.4 section A) and the shape-only row that both
//! `data/costs/` and `fieldsmith costs` are made of. Rows carry machine/shape fields
//! only — no path, no document name, no geometry, no field results; the one estimate a
//! row keeps is the rung-to-rung change the rung is about. Default location
//! `~/.local/state/fieldsmith/costs.jsonl` (`XDG_STATE_HOME` honoured),
//! `FIELDSMITH_COSTS_STORE` override for tests. Reading is always allowed (seeding);
//! nothing in this module writes anywhere but the store path it is handed.

use serde::{Deserialize, Serialize};

/// Store schema version — bump when a row field changes shape, so old rows stay readable.
pub const SCHEMA: u32 = 1;
/// Rotation cap: the store keeps its most recent `MAX_ROWS` rows.
pub const MAX_ROWS: usize = 512;

/// One shape-only rung record, ready for the store and for the `costs` CSV.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Row {
    pub schema: u32,
    /// Month-level timestamp, e.g. "2026-10".
    pub month: String,
    /// "cli" here; wasm/browser rows join the same shape later.
    pub platform: String,
    /// Engine identity (see [`engine_identity`]).
    pub engine: String,
    /// CPU model name (the "class" identifying whole rows); "unknown" off /proc.
    pub cpu: String,
    pub cores: usize,
    pub ram_gb: usize,
    /// Index in the run's schedule (0 = coarsest) — a run's block starts at `rung == 0`.
    pub rung: usize,
    /// The DOF count the rung aimed for (the schedule entry).
    pub target_dofs: usize,
    /// DOF the mesh actually built (provenance's own number).
    pub dofs: usize,
    pub elements: usize,
    /// Active field leaves of the document (the complexity proxy `fieldsmith costs` notes).
    pub field_leaves: usize,
    /// Host-measured wall time for the whole rung, ms.
    pub wall_ms: f64,
    /// Rung-to-rung deflection change (the error estimate), `None` on rung 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deflection_change_rel: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vm_change_rel: Option<f64>,
}

pub const CSV_HEADER: &str = "schema,month,platform,engine,cpu,cores,ram_gb,rung,target_dofs,dofs,elements,field_leaves,wall_ms,deflection_change_rel,vm_change_rel";

impl Row {
    /// The row as it would land in `data/costs/costs.csv` — the redaction preview.
    pub fn csv(&self) -> String {
        let opt = |v: Option<f64>| v.map(|x| x.to_string()).unwrap_or_default();
        [
            self.schema.to_string(),
            self.month.clone(),
            self.platform.clone(),
            quote(&self.engine),
            quote(&self.cpu),
            self.cores.to_string(),
            self.ram_gb.to_string(),
            self.rung.to_string(),
            self.target_dofs.to_string(),
            self.dofs.to_string(),
            self.elements.to_string(),
            self.field_leaves.to_string(),
            self.wall_ms.to_string(),
            opt(self.deflection_change_rel),
            opt(self.vm_change_rel),
        ]
        .join(",")
    }
}

/// Quote a CSV field when it carries anything a comma-splitter could mangle.
fn quote(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// fenics-rs workspace version (../fenics-rs/Cargo.toml — the sibling repo's version
/// constant; Cargo cannot read it across the path dependency, so it is pinned here and
/// a fenics-rs bump edits this line).
pub const FENICS_RS_VERSION: &str = "0.1.0";

/// The engine identity rows are stamped with — the engine that solved, not the host
/// (platform and cpu carry that). The CLI package version stands in for the whole
/// fieldsmith workspace, whose crates share one workspace version.
pub fn engine_identity() -> String {
    format!(
        "fieldsmith {} / fenics-rs {}",
        env!("CARGO_PKG_VERSION"),
        FENICS_RS_VERSION
    )
}

/// Where the store lives, or `None` when even `HOME` is missing. `FIELDSMITH_COSTS_STORE`
/// wins outright (tests point it at a temp path); else `$XDG_STATE_HOME/fieldsmith`,
/// else `$HOME/.local/state/fieldsmith`.
pub fn store_path() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("FIELDSMITH_COSTS_STORE") {
        return Some(std::path::PathBuf::from(p));
    }
    if let Ok(d) = std::env::var("XDG_STATE_HOME") {
        return Some(
            std::path::PathBuf::from(d)
                .join("fieldsmith")
                .join("costs.jsonl"),
        );
    }
    std::env::var("HOME")
        .ok()
        .map(|h| std::path::PathBuf::from(h).join(".local/state/fieldsmith/costs.jsonl"))
}

/// The machine identity rows record — std only: `available_parallelism` for cores,
/// `/proc/cpuinfo` + `/proc/meminfo` on Linux, "unknown" CPU elsewhere.
#[derive(Clone, Debug, PartialEq)]
pub struct Machine {
    pub cpu: String,
    pub cores: usize,
    pub ram_gb: usize,
}

impl Machine {
    pub fn probe() -> Self {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0);
        let ram_kb = std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|s| {
                s.lines().find_map(|l| {
                    l.strip_prefix("MemTotal:")
                        .and_then(|r| r.trim().strip_suffix(" kB"))
                        .and_then(|n| n.trim().parse::<usize>().ok())
                })
            })
            .unwrap_or(0);
        Machine {
            cpu: proc_cpu_model(),
            cores,
            ram_gb: (ram_kb + (1 << 19)) >> 20, // kB → GiB, rounded
        }
    }
}

/// First "model name" in /proc/cpuinfo (the box is normally homogeneous; a heterogeneous
/// host would need per-core labels the shape-only row cannot carry — noted in the CSV
/// header, not solved here). Anything but Linux: "unknown".
fn proc_cpu_model() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| {
                    l.strip_prefix("model name")
                        .and_then(|r| r.strip_prefix("\t: ").or_else(|| r.strip_prefix(": ")))
                })
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Month string from Unix time — civil-calendar conversion, no dependencies.
/// (days_from_civil / civil_from_days, Howard Hinnant's public-domain date algorithms.)
pub fn month_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, _d) = civil_from_days((secs / 86_400) as i64);
    format!("{y}-{m:02}")
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Read every row; unreadable lines are skipped (a torn final write stays non-fatal —
/// the reader reports what it understood, the next append rewrites the file whole).
pub fn read_all(path: &std::path::Path) -> Vec<Row> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Append rows and rotate — read, extend, keep the newest `MAX_ROWS`, write whole.
/// Caller owns the path (tests pass an explicit one); a failure is returned, not
/// printed: the solve that produced the rows has already answered.
pub fn append(path: &std::path::Path, rows: &[Row]) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("costs store {}: {e}", path.display()))?;
    }
    let mut all = read_all(path);
    all.extend_from_slice(rows);
    if all.len() > MAX_ROWS {
        all.drain(..all.len() - MAX_ROWS);
    }
    let mut body = String::new();
    for r in &all {
        body.push_str(
            &serde_json::to_string(r).map_err(|e| format!("costs store row: {e}"))?,
        );
        body.push('\n');
    }
    std::fs::write(path, body).map_err(|e| format!("costs store {}: {e}", path.display()))
}

/// Median wall time over rows of the same machine class, engine, platform and target
/// DOF count — the seed for `estimate_for` when the current run has no fit yet (one
/// rung in). Matching keys: cpu, cores, engine, platform, target_dofs (the schedule
/// entry the projection is for; achieved DOF drifts with the mesher's floor and the
/// wall time difference is second order at that resolution). The machine comes from
/// the caller: rows are keyed to the machine class that measured them, and a class is
/// not transferable — seeding with a probed machine inside would make that
/// untestable (V18 records per-class rows and bands for exactly this reason).
pub fn seed_median(rows: &[Row], machine: &Machine, target_dofs: usize) -> Option<(f64, usize)> {
    let engine = engine_identity();
    let mut walls: Vec<f64> = rows
        .iter()
        .filter(|r| {
            r.target_dofs == target_dofs
                && r.cpu == machine.cpu
                && r.cores == machine.cores
                && r.engine == engine
                && r.platform == "cli"
                && r.wall_ms.is_finite()
                && r.wall_ms > 0.0
        })
        .map(|r| r.wall_ms)
        .collect();
    if walls.is_empty() {
        return None;
    }
    walls.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = walls.len();
    let median = if n % 2 == 1 {
        walls[n / 2]
    } else {
        (walls[n / 2 - 1] + walls[n / 2]) / 2.0
    };
    Some((median, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = if m > 2 { m - 3 } else { m + 9 };
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    fn row(rung: usize, target: usize, wall: f64) -> Row {
        Row {
            schema: SCHEMA,
            month: "2026-10".into(),
            platform: "cli".into(),
            engine: engine_identity(),
            cpu: "Test CPU".into(),
            cores: 8,
            ram_gb: 16,
            rung,
            target_dofs: target,
            dofs: target + 13, // the mesher overshoots a little
            elements: 100 + rung,
            field_leaves: 5,
            wall_ms: wall,
            deflection_change_rel: if rung == 0 { None } else { Some(-0.02) },
            vm_change_rel: if rung == 0 { None } else { Some(0.01) },
        }
    }

    #[test]
    fn month_is_the_civil_month_of_today() {
        let (y, m, d) = civil_from_days(days_from_civil(2026, 10, 13));
        assert_eq!((y, m, d), (2026, 10, 13));
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(month_now().len(), 7);
        assert!(month_now().chars().nth(4) == Some('-'));
    }

    #[test]
    fn csv_round_trip_and_escaping() {
        let r = row(1, 150_000, 1234.5);
        assert_eq!(Row::csv(&r).split(',').count(), CSV_HEADER.split(',').count());
        let mut c = row(0, 30_000, 100.0);
        c.cpu = "AMD Ryzen 7, 8 cores".into();
        assert!(Row::csv(&c).contains("\"AMD Ryzen 7, 8 cores\""));
        // A quoted header split would miscount; the shape promise is column-count parity.
        assert_eq!(Row::csv(&c).split(',').count(), CSV_HEADER.split(',').count() + 1);
    }

    #[test]
    fn append_rotates_to_the_newest_max_rows() {
        let dir = std::env::temp_dir().join(format!(
            "fieldsmith-store-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("costs.jsonl");
        let many: Vec<Row> = (0..600).map(|i| row(i % 3, 30_000, 100.0 + i as f64)).collect();
        append(&path, &many).unwrap();
        let read = read_all(&path);
        assert_eq!(read.len(), MAX_ROWS);
        assert_eq!(read[0].wall_ms, 100.0 + (600 - MAX_ROWS) as f64);
        // A second append reads what rotation left and extends it.
        append(&path, &[row(0, 30_000, 999.0)]).unwrap();
        let read = read_all(&path);
        assert_eq!(read.len(), MAX_ROWS);
        assert_eq!(read.last().unwrap().wall_ms, 999.0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn seed_median_matches_machine_class_engine_and_target() {
        let mut rows: Vec<Row> = vec![row(1, 150_000, 100.0), row(1, 150_000, 300.0)];
        rows.push(row(1, 150_000, 200.0));
        // Off-engine rows are excluded (the 1.0e9 wall would break the median), and an
        // off-class row never matches whatever its target.
        let mut other = row(1, 150_000, 1.0e9);
        other.engine = "fieldsmith 0 / fenics-rs 0".into();
        rows.push(other);
        let mut off = row(1, 750_000, 1.0e9);
        off.cpu = "Other CPU".into();
        off.target_dofs = 750_000;
        rows.push(off);
        let m = Machine { cpu: "Test CPU".into(), cores: 8, ram_gb: 16 };
        let (med, n) = seed_median(&rows, &m, 150_000).unwrap();
        assert_eq!(n, 3);
        assert_eq!(med, 200.0);
        assert!(seed_median(&rows, &m, 750_000).is_none());
        assert!(seed_median(&[], &m, 150_000).is_none());
        // The machine class is the caller's: the `row` helper pins a fake cpu, which
        // never matches this test process's real one (cores must agree too), so the
        // same rows must not seed a real machine's projection (V18: per-class rows,
        // per-class bands — nothing transfers between classes).
        let real = Machine::probe();
        if real.cpu != m.cpu {
            assert!(seed_median(&[row(1, 150_000, 100.0)], &real, 150_000).is_none());
        }
    }

    #[test]
    fn read_all_skips_torn_lines() {
        let dir = std::env::temp_dir().join("fieldsmith-store-torn");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("torn.jsonl");
        std::fs::write(
            &path,
            format!("{}\n{{\"schema\":1,\"trunc", serde_json::to_string(&row(0, 30_000, 1.0)).unwrap()),
        )
        .unwrap();
        assert_eq!(read_all(&path).len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}