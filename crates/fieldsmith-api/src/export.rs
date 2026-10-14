//! The export core (GOAL v0.4 section C): one path for triangle-mesh export — facts
//! and bytes in the result, formatting and file writes out. The CLI verb keeps its
//! prose over exactly these numbers; the MCP tool ships exactly these bytes.
//!
//! Timings are f64 milliseconds measured with the caller's clock using the CLI's
//! `(clock() - t0)` arithmetic; the CLI divides for display, so the printed numbers
//! are the same floats the pre-refactor prints carried.

use fieldsmith_core::fill;
use fieldsmith_core::{Clock, Resolution};
use fieldsmith_doc::{Document, NodeKind, Op};
use fieldsmith_field::expr;
use fieldsmith_field::volume::{self, VolumeEstimate};
use fieldsmith_mesh::{
    adaptive::{self, AdaptiveOpts, Sample},
    export::{surface, write_3mf_volumes, Defects, TriMesh, Volume3mf},
};

/// What the caller wants built and at what fineness.
pub struct ExportOpts {
    /// Triangle size (mm).
    pub h: f64,
    /// Adaptive dual contouring refines only near the surface, which is what makes the
    /// 0.1 mm export budget reachable; see V8 for the measured defect classes against
    /// this model (and the uniform mesher's, for comparison).
    pub adaptive: bool,
    /// Solid body even on fill documents (skip the graded re-solve).
    pub solid_only: bool,
    /// Build binary STL bytes.
    pub stl: bool,
    /// Build 3MF bytes.
    pub mf: bool,
}

/// Facts of one named volume in the per-node 3MF path (root-level additive nodes only).
pub struct ExportPart {
    pub id: String,
    pub material: String,
    pub verts: usize,
    pub tris: usize,
    pub volume: VolumeEstimate,
    pub ms: f64,
}

/// The graded re-solve a fill document's export runs before meshing.
pub struct ExportGraded {
    pub mass_g: f64,
    pub solid_mass_g: f64,
    pub ms: f64,
}

/// Everything the export measured and built. Part meshes are consumed into the 3MF
/// bytes and not returned; callers report facts, not geometry.
pub struct Export {
    pub h: f64,
    pub adaptive: bool,
    pub verts: usize,
    pub tris: usize,
    pub mesh_volume: f64,
    /// Reference pair at the mesh spacing (export.rs in the wasm crate explains why the
    /// coarser auto spacing's Richardson band misses the truth on TPMS fill fields).
    pub reference: VolumeEstimate,
    /// Mesh volume against the field reference, percent (NaN when the reference is 0).
    pub delta_percent: f64,
    pub defects: Defects,
    /// Mesh build, field-reference, and defect-scan times (display facts, not contract).
    pub mesh_ms: f64,
    pub ref_ms: f64,
    pub scan_ms: f64,
    /// Per-node facts, filled only when `opts.mf` and the one-object-per-node 3MF
    /// applies — the same condition under which the CLI prints the volume rows.
    pub parts: Vec<ExportPart>,
    pub graded: Option<ExportGraded>,
    pub stl: Option<Vec<u8>>,
    pub mf: Option<Vec<u8>>,
    /// Time to the artifacts (mesh, references, scanning, 3MF build), excluding the
    /// caller's file writes.
    pub ms: f64,
}

/// True when every non-suppressed top-level node is additive and shapes nothing beyond its
/// own geometry — the condition under which one object per node reconstructs the same solid
/// as the union. Suppressed nodes are skipped by the exporter either way, so they don't
/// disqualify.
///
/// A subtract/intersect node, a group shell/offset/mirror/pattern modifier, or a per-node
/// blend shapes the body *beyond* the nodes' own shapes, so no set of per-node volumes
/// reconstructs it: those exports stay the single-union body labelled with the document
/// material. (Moved here from the CLI's export_cmd when the export core moved.)
fn volumes_ok(nodes: &[fieldsmith_doc::Node]) -> bool {
    nodes.iter().all(|n| {
        n.suppressed
            || (n.op == Op::Add
                && n.blend == 0.0
                && match &n.kind {
                    NodeKind::Group { modifiers, .. } => modifiers.is_empty(),
                    _ => true,
                })
    })
}

/// Mesh one field over the padded bounds of `b`.
fn mesh_field(e: &expr::Expr, b_lo: [f64; 3], b_hi: [f64; 3], h: f64, adaptive: bool) -> TriMesh {
    if adaptive {
        adaptive::surface_adaptive(
            |p| {
                let d = expr::eval(e, p);
                Sample { v: d.v, g: d.g }
            },
            b_lo,
            b_hi,
            h,
            &AdaptiveOpts::default(),
        )
    } else {
        surface(|p: [f64; 3]| expr::eval(e, p).v, b_lo, b_hi, h)
    }
}

fn padded(lo: [f64; 3], hi: [f64; 3], h: f64) -> ([f64; 3], [f64; 3]) {
    (
        [lo[0] - h, lo[1] - h, lo[2] - h],
        [hi[0] + h, hi[1] + h, hi[2] + h],
    )
}

/// Export one document: mesh it, measure it against the field, build the requested
/// artifact bytes.
pub fn export(doc: &Document, opts: &ExportOpts, clock: Clock) -> Result<Export, String> {
    let h = opts.h;
    if !h.is_finite() || h <= 0.0 {
        return Err("h must be positive and finite".into());
    }
    let t_all = clock();

    let e = expr::compile_body(doc);
    let b = expr::bounds(&e);
    if b.is_empty() {
        return Err("document has no solid".into());
    }

    // Fill documents export the graded part (the fill is part of the design); `solid_only`
    // exports the body alone.
    let (graded, field) = if doc.fill.is_some() && !opts.solid_only {
        let t0 = clock();
        let (_, g) = fill::graded(doc, Resolution::Dofs(150_000), clock).map_err(|x| x.0)?;
        let ms = clock() - t0;
        let graded = ExportGraded {
            mass_g: g.mass_g,
            solid_mass_g: g.solid_mass_g,
            ms,
        };
        (Some(graded), expr::Expr::Filled(Box::new(g.fill)))
    } else {
        (None, e)
    };

    let t0 = clock();
    let (lo, hi) = padded(b.lo, b.hi, h);
    let tri = mesh_field(&field, lo, hi, h, opts.adaptive);
    let mesh_ms = clock() - t0;

    let t0 = clock();
    let v = volume::volume(&field, h);
    let ref_ms = clock() - t0;

    let v_mesh = tri.volume();
    let delta = if v.volume > 0.0 {
        (v_mesh - v.volume) / v.volume * 100.0
    } else {
        f64::NAN
    };

    let t0 = clock();
    let defects = tri.defects();
    let scan_ms = clock() - t0;

    // When the document is a plain stack of add-shaped nodes — nothing subtracted or
    // intersected, no group shell/offset/mirror/pattern modifiers, no per-node blends, no
    // exported fill — each node's own shape already is a volume of the part, so the 3MF
    // carries one object per node, each labelled with its material (the node's label, else
    // the document material). Slicers union the shipped objects, so the printed solid is
    // the same body, and per-volume labels travel with it; where two labelled volumes
    // overlap, the slicer's rule (first hit) states that overlap's material.
    let per_volume = (doc.fill.is_none() || opts.solid_only) && volumes_ok(&doc.root);

    let mut parts: Vec<ExportPart> = Vec::new();
    let mf = if opts.mf {
        let mut meshes: Vec<(String, String, String, TriMesh)> = Vec::new();
        if per_volume {
            for n in &doc.root {
                if n.suppressed {
                    continue;
                }
                let fe = expr::compile_node(doc, n);
                let nb = expr::bounds(&fe);
                if nb.is_empty() {
                    continue;
                }
                let (nlo, nhi) = padded(nb.lo, nb.hi, h);
                let t1 = clock();
                let part = mesh_field(&fe, nlo, nhi, h, opts.adaptive);
                let mat = n.material.clone().unwrap_or_else(|| doc.material.clone());
                let vol = volume::volume(&fe, h);
                let part_ms = clock() - t1;
                parts.push(ExportPart {
                    id: n.id.clone(),
                    material: mat.clone(),
                    verts: part.verts.len(),
                    tris: part.tris.len(),
                    volume: vol,
                    ms: part_ms,
                });
                // Root-level volumes have no ancestors, so their effective colour is their
                // own (a group passes its colour down through the per-volume carve).
                meshes.push((n.id.clone(), mat, n.color.clone().unwrap_or_default(), part));
            }
        }
        let bytes = if !meshes.is_empty() {
            let vols: Vec<Volume3mf> = meshes
                .iter()
                .map(|(id, mat, colour, m)| Volume3mf {
                    name: id,
                    material: mat,
                    color: colour,
                    mesh: m,
                })
                .collect();
            write_3mf_volumes(&vols)
        } else {
            tri.write_3mf(&doc.name, &doc.material)
        };
        Some(bytes)
    } else {
        None
    };

    let stl = if opts.stl {
        Some(tri.write_stl())
    } else {
        None
    };

    Ok(Export {
        h,
        adaptive: opts.adaptive,
        verts: tri.verts.len(),
        tris: tri.tris.len(),
        mesh_volume: v_mesh,
        reference: v,
        delta_percent: delta,
        defects,
        parts,
        graded,
        stl,
        mf,
        mesh_ms,
        ref_ms,
        scan_ms,
        ms: clock() - t_all,
    })
}
