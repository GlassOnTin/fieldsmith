//! Regions: named parts of the surface, resolved against the analysis mesh's boundary.

use crate::SetupError;
use fieldsmith_doc::{Document, Face};
use fieldsmith_field::expr::{self, eval_face, placed_shape};
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
        [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
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

/// Indices of the facets that belong to region `id`.
///
/// A facet belongs to a selector `shape.face` when its corners lie on that shape's placed surface,
/// its normal is parallel to the shape's surface normal, and the nearest face of the shape at its
/// centroid is the selected face. Corners of boundary facets are on the zero set to root-finding
/// precision, so the corner test can be tight; the centroid sits inside the chord of curved faces.
pub fn region_facets(doc: &Document, id: &str, mesh: &P2Mesh, facets: &[Facet], h: f64) -> Result<Vec<usize>, SetupError> {
    let region = doc.regions.iter().find(|r| r.id == id).ok_or_else(|| SetupError(format!("unknown region '{id}'")))?;
    let tol_corner = 1e-6 * h.max(1.0);
    let tol_centroid = 0.25 * h;
    let mut out = Vec::new();
    let shapes: Vec<(expr::Expr, Face)> = region
        .selectors
        .iter()
        .map(|s| {
            placed_shape(doc, &s.shape)
                .map(|e| (e, s.face))
                .ok_or_else(|| SetupError(format!("region '{id}' refers to '{}', which is not in the model", s.shape)))
        })
        .collect::<Result<_, _>>()?;
    for (i, f) in facets.iter().enumerate() {
        let corners = f.corners(mesh);
        let c = f.centroid(mesh);
        let n = f.normal(mesh);
        let hit = shapes.iter().any(|(e, face)| {
            if corners.iter().any(|&p| expr::eval(e, p).v.abs() > tol_corner) {
                return false;
            }
            let d = expr::eval(e, c);
            if d.v.abs() > tol_centroid {
                return false;
            }
            let gl = (d.g[0] * d.g[0] + d.g[1] * d.g[1] + d.g[2] * d.g[2]).sqrt();
            if gl == 0.0 || ((d.g[0] * n[0] + d.g[1] * n[1] + d.g[2] * n[2]) / gl).abs() < 0.7 {
                return false;
            }
            *face == Face::All || eval_face(e, c).1 == *face
        });
        if hit {
            out.push(i);
        }
    }
    Ok(out)
}
