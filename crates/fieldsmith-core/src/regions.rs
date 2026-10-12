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

// ---------------------------------------------------------------- face frame --

/// Face-local frame of a labelled region (M11): a least-squares plane through the
/// region's facet centroids gives the normal n; the world axis least aligned with n
/// projects onto the local x axis (ties break in x, y, z order); y = n × x, so
/// (x, y, n) is right-handed and n points outward. Coordinates in the frame are
/// millimetres measured from the facets' area-weighted centroid.
#[derive(Clone, Copy, Debug)]
pub struct FaceFrame {
    /// The fitted plane's point (region centroid, mm).
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    /// Unit outward normal of the fitted plane.
    pub n: [f64; 3],
    /// RMS distance of the centroids from the fitted plane (mm).
    pub rms: f64,
    /// Sided in-plane extent: sqrt(12·λmax/N) of the centroid scatter (mm).
    pub extent: f64,
}

/// A region whose plane-fit RMS exceeds this fraction of its in-plane extent is
/// refused for face-local loads (GOAL.md M11: refused by name, not silently flattened).
pub const FACE_FRAME_RMS_FRACTION: f64 = 0.1;

/// Eigen-decomposition of a symmetric 3×3 by cyclic Jacobi rotations: the eigenvalues,
/// descending, and the unit eigenvector for each (column i of the returned matrix).
pub fn jacobi3(a: &[[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut a = *a;
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let scale = (a[0][0].abs() + a[1][1].abs() + a[2][2].abs()).max(1e-300);
    for _ in 0..50 {
        let (p, q) = if a[0][1].abs() >= a[0][2].abs() && a[0][1].abs() >= a[1][2].abs() {
            (0usize, 1usize)
        } else if a[0][2].abs() >= a[1][2].abs() {
            (0, 2)
        } else {
            (1, 2)
        };
        let e = a[p][q];
        if e.abs() < 1e-16 * scale {
            break;
        }
        let tau = (a[q][q] - a[p][p]) / (2.0 * e);
        let t = if tau >= 0.0 {
            1.0 / (tau + (1.0 + tau * tau).sqrt())
        } else {
            1.0 / (tau - (1.0 + tau * tau).sqrt())
        };
        let c = 1.0 / (1.0 + t * t).sqrt();
        let s = t * c;
        for k in 0..3 {
            let (akp, akq) = (a[k][p], a[k][q]);
            a[k][p] = c * akp - s * akq;
            a[k][q] = s * akp + c * akq;
            let (vkp, vkq) = (v[k][p], v[k][q]);
            v[k][p] = c * vkp - s * vkq;
            v[k][q] = s * vkp + c * vkq;
        }
        let (row_p, row_q) = (a[p], a[q]);
        for k in 0..3 {
            a[p][k] = c * row_p[k] - s * row_q[k];
            a[q][k] = s * row_p[k] + c * row_q[k];
        }
    }
    let mut order: [usize; 3] = [0, 1, 2];
    order.sort_by(|&i, &j| a[j][j].total_cmp(&a[i][i]));
    // Eigenvectors sit in the ACCUMULATED ROTATION'S columns, not rows.
    (
        [
            a[order[0]][order[0]],
            a[order[1]][order[1]],
            a[order[2]][order[2]],
        ],
        [
            [v[0][order[0]], v[1][order[0]], v[2][order[0]]],
            [v[0][order[1]], v[1][order[1]], v[2][order[1]]],
            [v[0][order[2]], v[1][order[2]], v[2][order[2]]],
        ],
    )
}

/// Fit the face-local frame for one region's facet set. Refuses nothing here — the
/// caller checks the fit against `FACE_FRAME_RMS_FRACTION` and names the load.
pub fn face_frame(mesh: &P2Mesh, facets: &[Facet], fs: &[usize]) -> FaceFrame {
    let pts: Vec<[f64; 3]> = fs.iter().map(|&i| facets[i].centroid(mesh)).collect();
    let areas: Vec<f64> = fs.iter().map(|&i| facets[i].area(mesh)).collect();
    let area_sum: f64 = areas.iter().sum::<f64>().max(1e-300);
    let mut c = [0.0f64; 3];
    for (p, w) in pts.iter().zip(&areas) {
        for k in 0..3 {
            c[k] += w * p[k] / area_sum;
        }
    }
    let mut s = [[0.0f64; 3]; 3];
    for p in &pts {
        let d = [p[0] - c[0], p[1] - c[1], p[2] - c[2]];
        for r in 0..3 {
            for cc in 0..3 {
                s[r][cc] += d[r] * d[cc];
            }
        }
    }
    let (eig, vecs) = jacobi3(&s);
    let count = fs.len().max(1) as f64;
    let rms = (eig[2] / count).max(0.0).sqrt();
    let extent = (12.0 * eig[0] / count).max(0.0).sqrt();
    // Orient the normal outward: along the area-weighted mean of the facets' normals.
    let mut nbar = [0.0f64; 3];
    for (&i, w) in fs.iter().zip(&areas) {
        let m = facets[i].normal(mesh);
        for k in 0..3 {
            nbar[k] += w * m[k];
        }
    }
    let mut n = vecs[2];
    if dot(n, nbar) < 0.0 {
        n = n.map(|v| -v);
    }
    // The world axis least aligned with n becomes x (ties keep the earlier axis).
    let axes: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let axis = (0..3)
        .min_by(|&a, &b| dot(axes[a], n).abs().total_cmp(&dot(axes[b], n).abs()))
        .unwrap_or(0);
    let mut x = axes[axis];
    for k in 0..3 {
        x[k] -= dot(axes[axis], n) * n[k];
    }
    let xl = (x[0].powi(2) + x[1].powi(2) + x[2].powi(2))
        .sqrt()
        .max(1e-300);
    let x = x.map(|v| v / xl);
    // y = n × x keeps (x, y, n) right-handed.
    let y = cross(n, x);
    FaceFrame {
        origin: c,
        x,
        y,
        n,
        rms,
        extent,
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
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
                .map(|e| Rule {
                    e,
                    face: s.face,
                    patch: s.patch,
                    axis,
                })
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
                Some(p) => expr::eval_mesh_patch(&r.e, c)
                    .map(|q| q == p as u16)
                    .unwrap_or(false),
            }
        });
        if hit {
            out.push(i);
        }
    }
    Ok(out)
}
