//! The sweep CSV (GOAL v0.4 section D): one row per sweep point, the last rung's
//! measured numbers. The `api --csv` verb and the web Results lens both write this
//! format; same run, same bytes. Floats format like the costs store's rows
//! (`f64::to_string`, the shortest round-trip decimal) — the byte-identity promise is
//! the decimal-range promise: both the Rust and the lens-side formatters print
//! shortest round-trip, so values inside ±1e6 and above 1e-6 agree byte for byte, and
//! exponent-range values do not (named, not assumed — the lens e2e evidence checks the
//! actual columns).

use serde_json::Value;

pub const CSV_HEADER: &str =
    "value,dofs,elements,wall_ms,deflection_mm,vm_mpa,deflection_change_rel,vm_change_rel,status";

/// Build the sweep CSV from the response envelope's JSON. One row per point, in
/// envelope order: the point's `value`, its last rung record's `dofs`, `elements`,
/// `wall_ms` and tracked `deflection_mm`/`vm_mpa`, the last rung's change columns
/// (empty on a single-rung point — no baseline), and the point's verdict as `met`,
/// `refusal`, or `ok` (no tolerance declared; the envelope stays `ok` either way).
/// An envelope without a sweep answer — including `status: error` — is a named error:
/// `--csv` is a sweep-question option, and naming what failed is the honest reply.
pub fn sweep_csv(body: &Value) -> Result<String, String> {
    let sweep = body
        .get("sweep")
        .and_then(|s| s.as_object())
        .ok_or_else(|| {
            "sweep csv: the envelope carries no sweep answer \
             (--csv is a sweep-question option)"
                .to_string()
        })?;
    let param = sweep.get("param").and_then(|p| p.as_str()).unwrap_or("");
    let points = sweep
        .get("points")
        .and_then(|p| p.as_array())
        .ok_or_else(|| "sweep csv: the envelope's sweep has no points".to_string())?;

    let mut rows = vec![CSV_HEADER.to_string()];
    for p in points {
        let value = p.get("value").and_then(|v| v.as_f64());
        let out = p.get("outcome").and_then(|o| o.as_object());
        let (value, out) = match (value, out) {
            (Some(v), Some(o)) => (v, o),
            _ => {
                return Err(format!(
                    "sweep csv: a point of {param:?} carries no value/outcome — \
                     the envelope's shape is not what this builder reads"
                ))
            }
        };
        let last = out
            .get("records")
            .and_then(|r| r.as_array())
            .and_then(|r| r.last());
        let last = match last {
            Some(l) => l,
            None => {
                return Err(format!(
                    "sweep csv: a point of {param:?} has no rung records — \
                     nothing measured to write"
                ))
            }
        };
        let num = |key: &str| last.get(key).and_then(|v| v.as_f64());
        // integers print without a decimal point, like the costs store's rows
        let int = |key: &str| -> String {
            num(key)
                .map(|x| format!("{}", x as u64))
                .unwrap_or_default()
        };
        // change columns: absent on a single-rung point
        let change = last.get("change").and_then(|c| c.as_object());
        let ch = |key: &str| -> String {
            change
                .and_then(|c| c.get(key))
                .and_then(|v| v.as_f64())
                .map(|x| x.to_string())
                .unwrap_or_default()
        };
        let tracked = last
            .get("tracked")
            .and_then(|t| t.as_object())
            .ok_or_else(|| {
                format!("sweep csv: the last rung of {param:?} carries no tracked values")
            })?;
        let tk = |key: &str| -> String {
            tracked
                .get(key)
                .and_then(|v| v.as_f64())
                .map(|x| x.to_string())
                .unwrap_or_default()
        };
        let status = if out.get("met").map(|m| !m.is_null()) == Some(true) {
            "met"
        } else if out.get("unmet").map(|m| !m.is_null()) == Some(true) {
            "refusal"
        } else {
            "ok"
        };
        rows.push(
            [
                value.to_string(),
                int("dofs"),
                int("elements"),
                num("wall_ms").map(|x| x.to_string()).unwrap_or_default(),
                tk("deflection_mm"),
                tk("vm_mpa"),
                ch("deflection_rel"),
                ch("vm_rel"),
                status.to_string(),
            ]
            .join(","),
        );
    }
    rows.push(String::new()); // trailing newline
    Ok(rows.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::respond;
    use serde_json::json;

    const DOC: &str = "\
fieldsmith 1
name \"sweep csv\"
custommaterial steel200 name=\"Steel\" e=200000 nu=0.3 yield=250 density=7.85
material steel200
box beam size=(100, 10, 10) at=(50, 5, 5)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -100)";

    fn sweep_body() -> serde_json::Value {
        let req = json!({
            "apiv": 2,
            "doc": DOC,
            "question": {
                "kind": "sweep",
                "param": "beam.size.2",
                "values": [6.0, 10.0],
                "rungs": [2000, 4000],
            },
        })
        .to_string();
        serde_json::from_str(&respond(&req).body).expect("envelope parses")
    }

    #[test]
    fn one_row_per_point_last_rung_numbers() {
        let body = sweep_body();
        let csv = sweep_csv(&body).expect("csv");
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], CSV_HEADER);
        assert_eq!(lines.len(), 3, "header + two points");
        assert!(lines[1].starts_with("6,"), "{}", lines[1]);
        assert!(lines[2].starts_with("10,"), "{}", lines[2]);
        // Two rungs ran, so the change columns are present (non-empty).
        for row in [&lines[1], &lines[2]] {
            let cols: Vec<&str> = row.split(',').collect();
            assert_eq!(cols.len(), CSV_HEADER.split(',').count());
            assert!(!cols[7].is_empty(), "vm_change_rel present: {row}");
            assert_eq!(cols[8], "ok", "no tolerance declared: {row}");
            // dofs/elements are integers, wall/tracked floats — the format contract.
            assert!(cols[1].parse::<u64>().is_ok(), "dofs: {row}");
            assert!(cols[3].parse::<f64>().is_ok(), "wall_ms: {row}");
            assert!(cols[4].parse::<f64>().is_ok(), "deflection_mm: {row}");
        }
    }

    #[test]
    fn refusal_points_name_themselves_in_the_status_column() {
        let req = json!({
            "apiv": 2,
            "doc": DOC,
            "question": {
                "kind": "sweep",
                "param": "beam.size.2",
                "values": [6.0],
                "rungs": [2000, 4000],
                "tolerance": "deflection:0.01%",
            }
        })
        .to_string();
        let body = serde_json::from_str(&respond(&req).body).expect("envelope parses");
        let csv = sweep_csv(&body).expect("csv");
        let row = csv.lines().nth(1).expect("one point");
        let status = row.split(',').next_back().unwrap();
        assert!(status == "met" || status == "refusal", "verdict: {status}");
    }

    #[test]
    fn non_sweep_envelopes_are_a_named_error() {
        let req = json!({
            "apiv": 2,
            "doc": DOC,
            "question": {"kind": "solve", "dofs": 2000},
        })
        .to_string();
        let body = serde_json::from_str(&respond(&req).body).expect("envelope parses");
        let e = sweep_csv(&body).expect_err("no sweep");
        assert!(e.contains("--csv is a sweep-question option"), "{e}");
    }
}
