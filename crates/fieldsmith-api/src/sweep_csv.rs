//! The sweep CSV (GOAL v0.4 section D): one row per sweep point, the last rung's
//! measured numbers. The `api --csv` verb and the web Results lens both write this
//! format; same run, same bytes. Floats format like the costs store's rows
//! (`f64::to_string`, the shortest round-trip decimal), so the byte-identity promise
//! holds for every value a shortest-representation printer agrees on — the decimal
//! range ±1e6 above 1e-6, named rather than assumed (the lens e2e evidence checks the
//! actual columns).
//!
//! The rows build from the typed [`Response`] — the engine's own f64s, not from the
//! envelope string parsed back with serde_json. That parse is not correctly rounded:
//! measured 2026-10-14, the literal `0.39066861588109364` (the first point of the V23
//! sweep's own first run) parses 1 ulp low under serde_json's default float path, and
//! its shortest printed form shifts a digit. Reading the typed values is what keeps
//! this builder's cells byte-identical to the wire envelope and to what any
//! correctly rounding client (the lens's JSON.parse) computes from it.

use crate::schema::Response;

pub const CSV_HEADER: &str = "value,dofs,elements,wall_ms,deflection_mm,vm_mpa,compliance_nmm,deflection_change_rel,vm_change_rel,compliance_change_rel,status";

/// Build the sweep CSV from the response. One row per point, in response order: the
/// point's `value`, its last rung record's `dofs`, `elements`, `wall_ms` and tracked
/// `deflection_mm`/`vm_mpa`, the last rung's change columns (empty on a single-rung
/// point — no baseline), and the point's verdict as `met`, `refusal`, or `ok` (no
/// tolerance declared; the envelope stays `ok` either way). A response without a
/// sweep answer — including every error envelope — is a named error: `--csv` is a
/// sweep-question option, and naming what failed is the honest reply.
pub fn sweep_csv(resp: &Response) -> Result<String, String> {
    let sweep = resp.sweep.as_ref().ok_or(
        "sweep csv: the envelope carries no sweep answer (--csv is a sweep-question option)",
    )?;
    let param = &sweep.param;

    let mut rows = vec![CSV_HEADER.to_string()];
    for p in &sweep.points {
        let last = p.outcome.records.last().ok_or_else(|| {
            format!("sweep csv: a point of {param} at value {} has no rung records — nothing measured to write", p.value)
        })?;
        // change columns: absent on a single-rung point (no baseline)
        let chd = last.change.as_ref().map(|c| c.deflection_rel.to_string());
        let chv = last.change.as_ref().map(|c| c.vm_rel.to_string());
        let chc = last.change.as_ref().map(|c| c.compliance_rel.to_string());
        let status = if p.outcome.met.is_some() {
            "met"
        } else if p.outcome.unmet.is_some() {
            "refusal"
        } else {
            "ok"
        };
        rows.push(
            [
                p.value.to_string(),
                format!("{}", last.dofs as u64),
                format!("{}", last.elements as u64),
                last.wall_ms.to_string(),
                last.tracked.deflection_mm.to_string(),
                last.tracked.vm_mpa.to_string(),
                last.tracked.compliance_nmm.to_string(),
                chd.unwrap_or_default(),
                chv.unwrap_or_default(),
                chc.unwrap_or_default(),
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

    fn sweep_response(values: &str, extra: &str) -> Response {
        let req = format!(
            "{{\"apiv\": 2, \"doc\": {DOC:?}, \"question\": {{\"kind\": \"sweep\", \
             \"param\": \"beam.size.2\", \"values\": {values}, \"rungs\": [2000, 4000]{extra}}}}}"
        );
        respond(&req)
            .response
            .expect("a parsed request answers with a typed envelope")
    }

    #[test]
    fn one_row_per_point_last_rung_numbers() {
        let resp = sweep_response("[6.0, 10.0]", "");
        let csv = sweep_csv(&resp).expect("csv");
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], CSV_HEADER);
        assert_eq!(lines.len(), 3, "header + two points");
        assert!(lines[1].starts_with("6,"), "{}", lines[1]);
        assert!(lines[2].starts_with("10,"), "{}", lines[2]);
        // Two rungs ran, so the change columns are present (non-empty).
        for row in [&lines[1], &lines[2]] {
            let cols: Vec<&str> = row.split(',').collect();
            assert_eq!(cols.len(), CSV_HEADER.split(',').count());
            assert!(!cols[7].is_empty(), "deflection_change_rel present: {row}");
            assert!(!cols[8].is_empty(), "vm_change_rel present: {row}");
            assert!(!cols[9].is_empty(), "compliance_change_rel present: {row}");
            assert!(cols[6].parse::<f64>().is_ok(), "compliance_nmm: {row}");
            assert_eq!(cols[10], "ok", "no tolerance declared: {row}");
            // dofs/elements are integers, wall/tracked floats — the format contract.
            assert!(cols[1].parse::<u64>().is_ok(), "dofs: {row}");
            assert!(cols[3].parse::<f64>().is_ok(), "wall_ms: {row}");
            assert!(cols[4].parse::<f64>().is_ok(), "deflection_mm: {row}");
        }
    }

    #[test]
    fn refusal_points_name_themselves_in_the_status_column() {
        let resp = sweep_response("[6.0]", ", \"tolerance\": \"deflection:0.01%\"");
        let csv = sweep_csv(&resp).expect("csv");
        let row = csv.lines().nth(1).expect("one point");
        let status = row.split(',').next_back().unwrap();
        assert!(status == "met" || status == "refusal", "verdict: {status}");
    }

    #[test]
    fn non_sweep_envelopes_are_a_named_error() {
        let req = format!(
            "{{\"apiv\": 2, \"doc\": {DOC:?}, \"question\": {{\"kind\": \"solve\", \"dofs\": 2000}}}}"
        );
        let resp = respond(&req)
            .response
            .expect("a parsed request answers with a typed envelope");
        let e = sweep_csv(&resp).expect_err("no sweep");
        assert!(e.contains("--csv is a sweep-question option"), "{e}");
    }

    /// The byte-identity promise, cell for cell: the CSV's tracked digits equal the
    /// wire envelope's digits, which is what any correctly rounding client (the lens's
    /// JSON.parse) prints back. Tracked is the record's last field in the struct order
    /// the compact envelope prints, so walking the `"tracked":{` occurrences by
    /// accumulated record counts lands on the exact literals.
    #[test]
    fn csv_cells_carry_the_wire_envelopes_digits() {
        let req = format!(
            "{{\"apiv\": 2, \"doc\": {DOC:?}, \"question\": {{\"kind\": \"sweep\", \
             \"param\": \"beam.size.2\", \"values\": [0.8, 10.0], \"rungs\": [2000, 4000]}}}}"
        );
        let reply = respond(&req);
        let resp = reply.response.as_ref().expect("typed envelope");
        let csv = sweep_csv(resp).expect("csv");
        let lines: Vec<&str> = csv.lines().collect();
        let needle = "\"tracked\":{\"deflection_mm\":";
        let mut walked = 0usize;
        for (i, p) in resp
            .sweep
            .as_ref()
            .expect("a csv request answered a sweep")
            .points
            .iter()
            .enumerate()
        {
            walked += p.outcome.records.len();
            let mut pos = 0usize;
            for _ in 0..walked {
                let idx = reply.body[pos..]
                    .find(needle)
                    .expect("tracked literal on the wire");
                pos += idx + needle.len();
            }
            let d_end = reply.body[pos..].find(',').expect("deflection field end");
            let wire_d = &reply.body[pos..pos + d_end];
            let vm_needle = ",\"vm_mpa\":";
            let vm_idx = reply.body[pos + d_end..]
                .find(vm_needle)
                .expect("vm literal follows deflection inside tracked");
            let vm_start = pos + d_end + vm_idx + vm_needle.len();
            let v_end = reply.body[vm_start..].find(',').expect("vm literal ends");
            let wire_v = &reply.body[vm_start..vm_start + v_end];
            let cols: Vec<&str> = lines[i + 1].split(',').collect();
            assert_eq!(
                cols[4], wire_d,
                "deflection cell vs wire literal (point {i})"
            );
            assert_eq!(cols[5], wire_v, "vm cell vs wire literal (point {i})");
        }
    }

    /// The measured drift, pinned. The literal `0.39066861588109364` — the V23
    /// sweep's own first run — parses 1 ulp low through a serde_json Value round
    /// trip and its shortest printed form shifts a digit; the same f64 through the
    /// typed path prints the whole literal back. Stamp the literal into a live
    /// response and hold both halves: the typed cell must carry the digits, and a
    /// Value parse of the same envelope must not — so a future change that rebuilds
    /// the rows through a Value parse fails here, not in a downstream handoff.
    #[test]
    fn the_typed_path_holds_the_drifting_literals_digits() {
        let mut resp = sweep_response("[6.0]", "");
        let d: f64 = "0.39066861588109364".parse().unwrap();
        let rec = resp.sweep.as_mut().unwrap().points[0]
            .outcome
            .records
            .last_mut()
            .unwrap();
        rec.tracked.deflection_mm = d;
        let csv = sweep_csv(&resp).expect("csv");
        assert!(csv.contains(",0.39066861588109364,"), "{csv}");
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        let cell = v["sweep"]["points"][0]["outcome"]["records"][1]["tracked"]["deflection_mm"]
            .to_string();
        assert_eq!(
            v["sweep"]["points"][0]["outcome"]["records"]
                .as_array()
                .map(|a| a.len()),
            Some(2),
            "the fixture's last record is index 1"
        );
        assert_eq!(
            cell, "0.3906686158810936",
            "the measured drift shifted shape"
        );
        assert_ne!(cell, "0.39066861588109364");
    }
}
