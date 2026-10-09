//! Regions: named parts of the surface, resolved against the analysis mesh's boundary.

use crate::SetupError;
use fieldsmith_doc::{Document, Face, NodeKind};
use fieldsmith_field::expr::{self, eval_face, placed_shape, Rigid};
use fieldsmith_field::meshfield::FACE_CONE_COS;
use fieldsmith_solve::P2Mesh;

/// A boundary triangle of the P2 mesh: corners (outward order), then midsides 01, 12, 20.
#[derive(Clone, Copy, Debug)]
pub struct Facet {
    pub nodes: [u32; 6],
}

impl Facet {
    fn corners(&self, m: &P2Mesh) -> [[f64; 3]; 3] {
        [0, 1, 2].map(|i| m.nodes[self.nodes[i] as usize])
    }
    fn cross(&self, m: &P2Mesh) -> [f64; 3] {
        let [a, b, c] = self.corners(m);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ]
    }
    pub fn area(&self, m: &P2Mesh) -> f64 {
        let n = self.cross(m);
        0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
    }
    /// Outward unit normal.
    pub fn normal(&self, m: &P2Mesh) -> [f64; 3] {
        let n = self.cross(m);
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-300);
        n.map(|c| c / l)
    }
    pub fn centroid(&self, m: &P2Mesh) -> [f64; 3] {
        let [a, b, c] = self.corners(m);
        [0, 1, 2].map(|k| (a[k] + b[k] + c[k]) / 3.0)
    }
}

/// One selector's compiled rule. `axis` is the face-rule's unit world direction (the axis
/// face on a mesh node, mapped through the node placement); `None` keeps the exact
/// primitive face test.
struct Rule {
    e: expr::Expr,
    face: Face,
    patch: Option<u32>,
    axis: Option<[f64; 3]>,
}

/// Indices of the facets that belong to region `id`.
///
/// A facet belongs to a selector `shape.face` when its corners lie on that shape's placed surface,
/// its normal is parallel to the shape's surface normal, and the nearest face of the shape at its
/// centroid is the selected face. A `.p3` selector (`patch`, with face `All`) further requires the
/// facet's centroid to sit on that surface patch of the named mesh; primitive faces carry no
/// patches and never claim one. Corners of boundary facets are on the zero set to root-finding
/// precision, so the corner test can be tight; the centroid sits inside the chord of curved faces.
pub fn region_facets(
    doc: &Document,
    id: &str,
    mesh: &P2Mesh,
    facets: &[Facet],
    h: f64,
) -> Result<Vec<usize>, SetupError> {
    let region = doc
        .regions
        .iter()
        .find(|r| r.id == id)
        .ok_or_else(|| SetupError(format!("unknown region '{id}'")))?;
    let tol_corner = 1e-6 * h.max(1.0);
    let tol_centroid = 0.25 * h;
    let mut out = Vec::new();
    let shapes: Vec<Rule> = region
        .selectors
        .iter()
        .map(|s| {
            // An axis face on a mesh node selects by outward-normal cone (the geometric face
            // rule); on primitives and for `all`/`side` the exact face test stays in charge.
            // An axis face on an extend node also selects by cone: the extend's derived
            // leaf carries a face hint (the swept surface's axis), so `region_facets`
            // matches the facet's outward normal against it — the same rule as a mesh,
            // closing V10's gap that a Face on an Extend node fell through to the
            // primitive branch.
            let axis = s.face.axis().and_then(|a| {
                doc.find(&s.shape).and_then(|n| match &n.kind {
                    NodeKind::Mesh { .. } => Some(Rigid::from_doc(&n.transform).grad_to_world(a)),
                    NodeKind::Extend { .. } => Some(a),
                    _ => None,
                })
            });
            placed_shape(doc, &s.shape)
                .map(|e| Rule { e, face: s.face, patch: s.patch, axis })
                .ok_or_else(|| {
                    SetupError(format!(
                        "region '{id}' refers to '{}', which is not in the model",
                        s.shape
                    ))
                })
        })
        .collect::<Result<_, _>>()?;
    for (i, f) in facets.iter().enumerate() {
        let corners = f.corners(mesh);
        let c = f.centroid(mesh);
        let n = f.normal(mesh);
        let hit = shapes.iter().any(|r: &Rule| {
            if corners
                .iter()
                .any(|&p| expr::eval(&r.e, p).v.abs() > tol_corner)
            {
                return false;
            }
            let d = expr::eval(&r.e, c);
            if d.v.abs() > tol_centroid {
                return false;
            }
            let gl = (d.g[0] * d.g[0] + d.g[1] * d.g[1] + d.g[2] * d.g[2]).sqrt();
            if gl == 0.0 || ((d.g[0] * n[0] + d.g[1] * n[1] + d.g[2] * n[2]) / gl).abs() < 0.7 {
                return false;
            }
            match r.axis {
                // Mesh face rule: the facet's outward normal within the face cone (same
                // 45° threshold as the sweep mask — a labelled support and a swept skirt
                // agree). Primitives never land here.
                Some(ax) => {
                    if n[0] * ax[0] + n[1] * ax[1] + n[2] * ax[2] < FACE_CONE_COS {
                        return false;
                    }
                }
                None => {
                    if !(r.face == Face::All || eval_face(&r.e, c).1 == r.face) {
                        return false;
                    }
                }
            }
            match r.patch {
                None => true,
                Some(p) => expr::eval_mesh_patch(&r.e, c).map(|q| q == p as u16).unwrap_or(false),
            }
        });
        if hit {
            out.push(i);
        }
    }
    Ok(out)
}
