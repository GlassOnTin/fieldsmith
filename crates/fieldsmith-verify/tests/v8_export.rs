//! V8 — export of the filled bracket (GOAL.md): the field the Make lens ships is the same
//! one measured here. Meshes the graded fill at the Make lens's default spacing, compares
//! the mesh volume against a converged field integral, counts surface defects, rounds the
//! STL through its reader, and inflates the 3MF's model bytes to confirm the metadata.
//! Writes docs/verification/V8.md before asserting.
//!
//! Scope, stated in the report: the package checks run on the uniform mesh at h = 0.25 mm.
//! The browser export's default is adaptive dual contouring (finer only near the surface),
//! measured in its own section here at the same spacing; it is the only route to the GOAL
//! budget "graded bracket at 0.1 mm feature resolution in ≤ 15 s", which the uniform
//! mesher misses at 411 s native. The uniform mesher keeps the fill surface's classified
//! non-manifold zipping class (docs/verification/V8.md, section "Non-manifold edges"); the
//! adaptive stitch separates surface sheets per leaf — one vertex per sheet, and a
//! head-of-weld chord registry that draws each weld chord once — and PrusaSlicer 2.9.6
//! imports its graded 3MF as `manifold = yes` at both probed spacings (measured in
//! `slicer.md`). The plain solid is defect-free on the uniform path at every probed
//! spacing (`v8_probe3.rs`); on the adaptive path the CLI scan reports the plain solid
//! defect-free at h 0.1 after the separation (`adaptive_plain_probe.rs` measured 51
//! crease-cell pairs before it).

use fieldsmith_core::fill::graded;
use fieldsmith_core::Resolution;
use fieldsmith_field::{expr, volume, V3};
use fieldsmith_mesh::adaptive::{surface_adaptive, AdaptiveOpts, Sample};
use fieldsmith_mesh::export::{read_stl, surface};
use fieldsmith_verify::{example, Check, Report};
use std::time::Instant;

fn clock() -> f64 {
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

/// The field reference integral at this spacing measured ± 12.6 mm³ (0.05 %) in
/// `v8_probe.rs` (fine convergence ladder 1.5 → 0.094 with Richardson error).
const FIELD_H: f64 = 0.094;
/// Make lens's default export spacing (mm).
const MESH_H: f64 = 0.25;
/// GOAL.md: mesh volume within 1 % of the field reference.
const VOL_TOL: f64 = 0.01;

/// One central-directory entry: (filename, uncompressed bytes through raw-deflate inflate).
/// The package is self-produced, so only the fields this writer emits are exercised.
fn zip_entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut i = bytes.len();
    while i >= 4 {
        if &bytes[i - 4..i] == b"PK\x05\x06" {
            break;
        }
        i -= 1;
    }
    assert!(i >= 4, "3MF zip has no end-of-central-directory record");
    let eocd = i - 4;
    let count = u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]) as usize;
    let cd_at = u32::from_le_bytes([
        bytes[eocd + 16],
        bytes[eocd + 17],
        bytes[eocd + 18],
        bytes[eocd + 19],
    ]) as usize;
    let mut out = Vec::new();
    let mut p = cd_at;
    for _ in 0..count {
        assert_eq!(
            &bytes[p..p + 4],
            b"PK\x01\x02",
            "central directory signature"
        );
        let csize = u32::from_le_bytes([bytes[p + 20], bytes[p + 21], bytes[p + 22], bytes[p + 23]])
            as usize;
        let usize_ =
            u32::from_le_bytes([bytes[p + 24], bytes[p + 25], bytes[p + 26], bytes[p + 27]])
                as usize;
        let nlen = u16::from_le_bytes([bytes[p + 28], bytes[p + 29]]) as usize;
        let elen = u16::from_le_bytes([bytes[p + 30], bytes[p + 31]]) as usize;
        let clen = u16::from_le_bytes([bytes[p + 32], bytes[p + 33]]) as usize;
        let lho = u32::from_le_bytes([bytes[p + 42], bytes[p + 43], bytes[p + 44], bytes[p + 45]])
            as usize;
        let name = String::from_utf8(bytes[p + 46..p + 46 + nlen].to_vec()).unwrap();
        p += 46 + nlen + elen + clen;
        // Local header: skip its variable fields to the payload.
        let nlen2 = u16::from_le_bytes([bytes[lho + 26], bytes[lho + 27]]) as usize;
        let elen2 = u16::from_le_bytes([bytes[lho + 28], bytes[lho + 29]]) as usize;
        let data_at = lho + 30 + nlen2 + elen2;
        let body = &bytes[data_at..data_at + csize];
        let data = miniz_oxide::inflate::decompress_to_vec(body).expect("3MF entry inflates");
        assert_eq!(data.len(), usize_, "entry uncompressed size");
        out.push((name, data));
    }
    out
}

#[test]
fn v8_export_the_bracket() {
    let t0 = Instant::now();
    let doc = example("bracket-filled.fsm");
    let (_, g) = graded(&doc, Resolution::Dofs(150_000), clock).unwrap();
    let build_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let filled = expr::Expr::Filled(Box::new(g.fill));
    let v_field = volume::integrate(&filled, FIELD_H, None);
    println!(
        "  field reference {:.1} mm³ (build {:.1} s)",
        v_field,
        build_ms / 1000.0
    );

    let b = expr::bounds(&filled);
    let lo = [b.lo[0] - MESH_H, b.lo[1] - MESH_H, b.lo[2] - MESH_H];
    let hi = [b.hi[0] + MESH_H, b.hi[1] + MESH_H, b.hi[2] + MESH_H];
    let t = Instant::now();
    let mesh = surface(|p: V3| expr::eval(&filled, p).v, lo, hi, MESH_H);
    let mesh_ms = t.elapsed().as_secs_f64() * 1000.0;
    println!(
        "  mesh {} verts {} tris ({:.1} s)",
        mesh.verts.len(),
        mesh.tris.len(),
        mesh_ms / 1000.0
    );

    let v_mesh = mesh.volume();
    let d = mesh.defects();
    let t = Instant::now();
    let stl = mesh.write_stl();
    let re = read_stl(&stl).expect("written STL parses");
    let stl_ms = t.elapsed().as_secs_f64() * 1000.0;
    let v_stl = re.volume();
    let stl_pct = 100.0 * (v_stl - v_mesh) / v_mesh;
    println!(
        "  STL round trip {:.1} ms: volume {v_stl:.1} mm³ ({stl_pct:+.4} %)",
        stl_ms
    );

    let bytes = mesh.write_3mf(&doc.name, &doc.material);
    let entries = zip_entries(&bytes);
    let model = entries
        .iter()
        .find(|(n, _)| n.ends_with(".model"))
        .expect("3MF carries a .model entry");
    let model_text = String::from_utf8_lossy(&model.1).into_owned();
    assert!(model_text.contains("<mesh"), "3MF model carries the mesh");
    assert!(
        model_text.contains(&doc.name),
        "3MF model carries the part name"
    );
    assert!(
        model_text.contains(&doc.material),
        "3MF model carries the material"
    );
    let vert_tag_count = model_text.matches("<vertex ").count();
    let tri_tag_count = model_text.matches("<triangle v").count();
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    println!("  3MF entries {names:?}: {vert_tag_count} vertices, {tri_tag_count} triangles");
    assert_eq!(
        vert_tag_count,
        mesh.verts.len(),
        "3MF vertex count matches the mesh"
    );
    assert_eq!(
        tri_tag_count,
        mesh.tris.len(),
        "3MF triangle count matches the mesh"
    );
    assert!(
        names.iter().any(|n| n.ends_with("[Content_Types].xml")),
        "3MF package typed"
    );

    // The slice plane must trace the exact export mesh: mid-height cut around each z cell
    // has the same total outline as the STL round-trip's own slice.
    let zmid = (b.lo[2] + b.hi[2]) / 2.0;
    let segs = mesh.slice_at(zmid);
    assert!(
        !segs.is_empty(),
        "a mid-height slice of the bracket has sections"
    );
    let outline = |segs: &[f64]| -> f64 {
        let mut sum = 0.0;
        for k in 0..segs.len() / 4 {
            sum += (segs[4 * k + 2] - segs[4 * k]).hypot(segs[4 * k + 3] - segs[4 * k + 1]);
        }
        sum
    };
    let l0 = outline(&segs);
    let segs2 = re.slice_at(zmid);
    let l1 = outline(&segs2);
    assert!(
        (l1 - l0).abs() <= 1e-6 * l0.max(1.0),
        "STL round trip slices differently: {l1} vs {l0} mm total outline"
    );
    println!(
        "  slice at z {zmid}: {} segments, total length {l0:.1} mm (round trip agrees to {l1:.6})",
        segs.len() / 4
    );

    // The adaptive path the browser export uses, measured on the same field at the same
    // spacing. The counts below travel into the report section; the 0.1 mm budget run is
    // `adaptive_probe.rs` (ignored, heavy).
    let t = Instant::now();
    let amesh = surface_adaptive(
        |p: V3| {
            let d = expr::eval(&filled, p);
            Sample { v: d.v, g: d.g }
        },
        lo,
        hi,
        MESH_H,
        &AdaptiveOpts::default(),
    );
    let amesh_ms = t.elapsed().as_secs_f64() * 1000.0;
    let av_mesh = amesh.volume();
    let ad = amesh.defects();
    println!(
        "  adaptive mesh {} verts {} tris ({:.1} s) volume {av_mesh:.1} mm³; defects {ad:?}",
        amesh.verts.len(),
        amesh.tris.len(),
        amesh_ms / 1000.0
    );

    let mut r = Report::new(
        "V8",
        "Export of the filled bracket: volume, defects, package integrity",
    );
    r.section(
        "Method",
        format!(
            "The document `examples/bracket-filled.fsm` (name \"{}\", material `{}`) is built through the same \
             `graded()` engine the Make lens calls, then handed to `fieldsmith-mesh::export::surface` at h = {MESH_H} mm. \
             The volume reference is a field integral of the SAME graded field at h = {FIELD_H} mm — the probe \
             (`v8_probe.rs`, release) measured its fine-reference ladder at 23484.2 ± 12.6 mm³ there (0.05 %), so the \
             field side of the comparison is fixed independently before the mesh is built. Defects come from \
             `TriMesh::defects` (edge-map scan); self-intersections from the spatial-hash scan. The STL is written and \
             read back; the 3MF is unpacked (central directory + raw-deflate inflate) and its model XML checked.",
            doc.name, doc.material
        ),
    );
    r.check(Check::relative(
        "mesh volume vs field volume",
        v_field,
        v_mesh,
        "mm³",
        VOL_TOL,
    ));
    r.check(Check::absolute(
        "open edges",
        0.0,
        d.open_edges as f64,
        "",
        0.0,
    ));
    r.check(Check::absolute(
        "orientation defects",
        0.0,
        d.orientation_defects as f64,
        "",
        0.0,
    ));
    r.check(Check::absolute(
        "degenerate triangles",
        0.0,
        d.degenerate_triangles as f64,
        "",
        0.0,
    ));
    r.check(Check::absolute(
        "self-intersections",
        0.0,
        d.self_intersections as f64,
        "",
        0.0,
    ));
    r.check(Check::relative(
        "STL round-trip volume",
        v_mesh,
        v_stl,
        "mm³",
        0.002,
    ));
    r.check(Check::relative(
        "adaptive mesh volume vs field volume",
        v_field,
        av_mesh,
        "mm³",
        VOL_TOL,
    ));

    r.section(
        "Non-manifold edges of the graded fill",
        format!(
            "{} edges with ≥ 3 incident triangles at h = {MESH_H}. The classification probe (`v8_probe2.rs`) walks 26 \
             directions per endpoint through the field: {} of {} endpoints carry vanishing material (true pinch \
             points of the fill surface, thickness → 0); every other endpoint keeps healthy interior either side \
             (median span ≥ 1.2 mm, p10 ≥ 0.35 mm — two surface sheets crossing within one cell, material intact). \
             No zeroed lattice nodes. Refinement does not drive the count to 0 (2277 at h 0.25, 2197 at 0.15, \
             547 at 0.10 in probe 1; counts move but persist), so the boundary is honest: this is \
             offset-surface TPMS at printable wall thickness with cell-width zipping. The control (`v8_probe3.rs`) \
             meshes the plain solid bracket with the same mesher and reports ZERO defects of every class — \
             non-manifold included — at h 0.25/0.15/0.10 (4.7/21.0/70.4 M verts; volume +0.006 %/+0.003 %/+0.006 % \
             off the field auto-integral). The defects are therefore owned by the fill surface, not the mesher. The \
             classification is the uniform path's; the adaptive export path below implements that named work — \
             per-sheet vertices plus a head-of-weld chord registry — and measures {} residual non-manifold edges on \
             the same model at h = {MESH_H}. Slicers take the solid as the union of the closed shell, which these \
             crossings leave intact.",
            d.non_manifold_edges, 43usize, 3403usize, ad.non_manifold_edges
        ),
    );
    r.section(
        "Adaptive dual contouring (the browser export path)",
        format!(
            "{} verts, {} triangles at h = {MESH_H}, meshed in {:.2} s (uniform: {:.1} s; volume {:.1} mm³, \
             {:.3} % off the field reference). It refines to h only within the gradient certificate near the \
             surface, which is what makes the GOAL 0.1 mm export budget reachable: the release CLI measured \
             the 0.1 mm export — 9265984 verts, 18535066 triangles, mesh 8.5 s + 3MF write 0.08 s of export \
             path (the same run's verification re-solve and defect scan add ~18 s) — against ≤ 15 s, versus \
             the uniform mesher's 411 s native at the same spacing (probe 1). Its defect classes on the \
             graded fill are measured (`adaptive_defect_census.rs`): {} open edges, {} non-manifold edges, \
             {} degenerate triangles, {} self-intersecting pairs at h = {MESH_H}; the pre-separation census \
             at the same spacing measured 997 / 269 / 343 / 34414, and at h = 0.1 the CLI now measures \
             8 / 229 / 4 / 7320 (pre-separation 8200 / 1213 / 2810 / 383834). The two former fan classes \
             are closed by the stitches' one-vertex-per-sheet separation and chord registry: every \
             multi-sheet leaf carries one vertex per surface sheet, so a leaf's fan pairs sheet-to-sheet \
             instead of straddling a neighbouring sheet's tiles, and the weld registry — fan vertices \
             sorted by (position hash, index), one head kept per distinct bitwise position, chords keyed \
             by head pair — draws each chord between welded vertices exactly once, demoting any second \
             quad on the same chord to triangulate along its own crossings chord. The structure the fix \
             addresses is unchanged: 17598 of 3075633 leaves at h 0.25 carry two or more wall sheets in \
             one cell (QEF residual > 0.3 cell), 233113 at h 0.1. What remains at h = {MESH_H}: the {} \
             non-manifold edges are short spokes (10–220 µm) at lattice-aligned sheet-meeting vertices — \
             the same welded lattice-corner class the double-gyroid fixture counts at 6 — with the fans' \
             third corners ≤ 80 µm off the spoke axis; the {} self-intersecting pairs are interpenetrating \
             wall tiles between neighbouring sheets' contours (normal-angle bins perp/mid/coplanar \
             [166, 4778, 15]; the census's printed sample pairs sit 0.3–3.5 mm from the nearest residual \
             edge endpoint). The uniform mesher on the same field reports 2277 non-manifold edges and none \
             of the other classes. Refinement narrows these counts but does not erase them (the wall \
             planes are self-similar at every level). The plain solid control on this path reports zero \
             edges and geometry classes — the h 0.1 CLI scan measures it defect-free including \
             self-intersections; `adaptive_plain_probe.rs` measured 51 crease-cell pairs before the \
             separation.",
            amesh.verts.len(),
            amesh.tris.len(),
            amesh_ms / 1000.0,
            mesh_ms / 1000.0,
            av_mesh,
            (av_mesh - v_field) / v_field * 100.0,
            ad.open_edges,
            ad.non_manifold_edges,
            ad.degenerate_triangles,
            ad.self_intersections,
            ad.non_manifold_edges,
            ad.self_intersections,
        ),
    );
    r.section(
        "Slicer import (PrusaSlicer 2.9.6)",
        format!(
            "PrusaSlicer 2.9.6 (flathub) `--info` on the graded exports this test measures, CLI-driven \
             (measurement log in `slicer.md`): the h {MESH_H} package reads `manifold = yes` — 3026110 facets, \
             13 shells, Prusa volume 23465.2 mm³ (0.02 % off this mesh's {:.1}); the h 0.1 export reads \
             `manifold = yes` — 18535066 facets, 19 shells, 23637.5 mm³ (0.6 %, the same Prusa-volume offset \
             class as the pre-separation meshes). Both measured `manifold = no` before the separation (83 \
             and 546 open edges, 17 and 73 shells), so the repair-dialog trigger on open clears on the \
             graded part, not only the plain-solid control; on a clean import PrusaSlicer reports no \
             `open_edges` field. The {} non-manifold spokes and {} wall-tile pairs this mesh carries \
             coexist with that clean import — the slicer's weld accepts the closed shells as read.",
            av_mesh,
            ad.non_manifold_edges,
            ad.self_intersections,
        ),
    );
    r.section(
        "Timings",
        format!(
            "Build: {:.1} s (graded fill incl. re-solve). Uniform mesh: {:.1} s ({:.0} verts at h = {MESH_H}); \
             adaptive mesh: {:.1} s ({} verts). STL write+read: {:.1} ms. The uniform mesher does not meet the \
             GOAL 0.1 mm budget (3MF export ≤ 15 s): h = 0.1 measured 411 s natively in probe 1 (52 M verts; the \
             on-demand positions refactor cost ~50 % of mesh time there — 270 s before it — in exchange for a 389 \
             MiB smaller high-water mark). Adaptive dual contouring is the export path that meets it, measured in \
             the section above.",
            build_ms / 1000.0,
            mesh_ms / 1000.0,
            mesh.verts.len(),
            amesh_ms / 1000.0,
            amesh.verts.len(),
            stl_ms
        ),
    );
    r.finish();
}
