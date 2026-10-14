//! Regions: named parts of the surface, resolved against the analysis mesh's boundary.

use crate::SetupError;
use fieldsmith_doc::{Document, Face, NodeKind};
use fieldsmith_field::expr::{self, placed_shape, Rigid};
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

/// Compile one region's selectors into rules, mapping axis faces through the shape node's
/// placement (mismatched shapes are the caller's error to report with the region's name).
fn rules_for(doc: &Document, id: &str) -> Result<Vec<Rule>, SetupError> {
    let region = doc
        .regions
        .iter()
        .find(|r| r.id == id)
        .ok_or_else(|| SetupError(format!("unknown region '{id}'")))?;
    region
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
        .collect()
}

/// Whether region rules claim facet `f` by the exact chain. This is the per-selector test
/// as documented on [`region_facets_exact`], factored out so the second pass can find the
/// facets no region wants.
fn chain_hit(
    shapes: &[Rule],
    f: &Facet,
    mesh: &P2Mesh,
    tol_corner: f64,
    tol_centroid: f64,
) -> bool {
    let corners = f.corners(mesh);
    let c = f.centroid(mesh);
    let n = f.normal(mesh);
    shapes.iter().any(|r: &Rule| {
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
                if r.face != Face::All
                    && !corners
                        .iter()
                        .all(|&pt| expr::on_face(&r.e, pt, r.face, tol_corner))
                {
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
    })
}

/// Indices of the facets that belong to region `id` by the exact chain only. A facet
/// belongs to a selector `shape.face` when its corners lie on that shape's placed surface,
/// its normal is parallel to the shape's surface normal, and every corner sits on the selected
/// face's own carrier surface (`.bottom` = the cap plane, `.side` = the wall cylinder, a box axis
/// face = its plane) — not merely nearest to it. The carrier test is what keeps a facet that
/// straddles the edge between two named faces from being claimed by either: a wall facet whose
/// centroid sits one chord-sag above a cylinder's cap classifies Bottom at the centroid, but its
/// wall corners stand 0.1–1.5 mm off the cap plane and fail the carrier test (V20). A `.p3`
/// selector (`patch`, with face `All`) further requires the facet's centroid to sit on that
/// surface patch of the named mesh; primitive faces carry no patches and never claim one. Corners
/// of boundary facets are on the zero set to root-finding precision, so the corner and carrier
/// tests can be tight; the centroid sits inside the chord of curved faces.
///
/// This is the whole contract for supports and loads ([`crate::fixed_mask`],
/// [`crate::case_force_report`]): a support must fix only nodes standing exactly on its carrier.
/// Pinning a node the mesh put off the plane contradicts any exact solution with nonzero
/// displacement there — measured in V20's anchor, where second-pass claims on support regions
/// turned a free-expansion field (σ ≡ 0) into a 34 MPa one.
pub fn region_facets_exact(
    doc: &Document,
    id: &str,
    mesh: &P2Mesh,
    facets: &[Facet],
    h: f64,
) -> Result<Vec<usize>, SetupError> {
    let tol_corner = 1e-6 * h.max(1.0);
    let tol_centroid = 0.25 * h;
    let rules = rules_for(doc, id)?;
    Ok((0..facets.len())
        .filter(|&i| chain_hit(&rules, &facets[i], mesh, tol_corner, tol_centroid))
        .collect())
}

/// Indices of the facets that belong to region `id`, exact chain plus the second pass.
///
/// The exact chain ([`region_facets_exact`]) refuses boundary facets that approximate a named
/// surface while carrying a corner off its carrier (rim "chamfer" faces whose cap corner is one
/// lattice point onto the cap plane; wall facets whose outward corner is an interior lattice
/// point). For boundary DATA that refusal leaves a hole: a temperature region's chime facet
/// would take a free node, and with |∇T| ≈ 10 K/mm at V20's bore wall the free corner sags
/// 5–12 K off the prescribed profile. The second pass claims such a facet for the region whose
/// carrier is nearest its centroid, within the centroid tolerance, when the facet's outward
/// normal stays inside that carrier's own cone ([`carrier_normal`] — not the SDF gradient at
/// the centroid, which points through whichever surface is nearest there and on a plain box
/// aliases the named carrier through an unnamed neighbouring face). A cap-plane facet's normal
/// is square to a wall carrier and can never be stolen by it. Boundary data there is the exact
/// data at the carrier,
/// so claiming trades a hole in the region's Dirichlet for the mesh's own chord deviation from
/// the surface.
///
/// Regions referenced by a `support` statement are excluded from the second-pass race: their
/// facets stay exactly as the chain computes them ([`region_facets_exact`]). Support pins are
/// constraints, not data — a pin at a node the exact field moves (off the support plane) makes
/// the pipeline contradict its own exact solution, which is V20's measured anchor failure.
/// Loads likewise take the exact set. So the second pass fills Dirichlet-data holes only:
/// temperature regions, and the surface-region attribution pass in `prepare` (labelling, not
/// physics). Carriers of every other racing region are in the same race, in document order on
/// ties, so no region can steal a facet another region's carrier rides closer to.
pub fn region_facets(
    doc: &Document,
    id: &str,
    mesh: &P2Mesh,
    facets: &[Facet],
    h: f64,
) -> Result<Vec<usize>, SetupError> {
    let mut out = region_facets_exact(doc, id, mesh, facets, h)?;
    let tol_corner = 1e-6 * h.max(1.0);
    let tol_centroid = 0.25 * h;
    let id_index = doc.regions.iter().position(|r| r.id == id);

    // Regions pinned by a support statement never race: their chime facets stay free.
    let no_race: Vec<&str> = doc.supports.iter().map(|s| s.region.as_str()).collect();
    let region_rules: Vec<Result<Vec<Rule>, SetupError>> =
        doc.regions.iter().map(|r| rules_for(doc, &r.id)).collect();
    let mut claimed_elsewhere = vec![false; facets.len()];
    for shapes in region_rules.iter().flatten() {
        for (i, f) in facets.iter().enumerate() {
            if !claimed_elsewhere[i] && chain_hit(shapes, f, mesh, tol_corner, tol_centroid) {
                claimed_elsewhere[i] = true;
            }
        }
    }
    for (i, f) in facets.iter().enumerate() {
        if claimed_elsewhere[i] {
            continue;
        }
        let c = f.centroid(mesh);
        let n = f.normal(mesh);
        let mut best: Option<(usize, f64)> = None;
        for (ri, result) in region_rules.iter().enumerate() {
            if no_race.contains(&doc.regions[ri].id.as_str()) {
                continue;
            }
            let shapes = match result {
                Ok(shapes) => shapes,
                Err(_) => continue,
            };
            for r in shapes {
                let Some(gap) = expr::face_carrier_gap(&r.e, r.face, c) else {
                    continue;
                };
                if gap > tol_centroid {
                    continue;
                }
                let Some(cn) = carrier_normal(&r.e, r.face, c) else {
                    continue;
                };
                if cn[0] * n[0] + cn[1] * n[1] + cn[2] * n[2] < 0.7 {
                    continue;
                }
                if best.is_none_or(|(_, bg)| gap < bg) {
                    best = Some((ri, gap));
                }
            }
        }
        if let Some((ri, _)) = best {
            if Some(ri) == id_index {
                out.push(i);
            }
        }
    }
    Ok(out)
}

/// Numeric unit outward normal of the named-face carrier at `p`, by central differences of
/// [`expr::face_carrier_signed`] across the placed shape's copies. The carriers are exact
/// planes and cylinders (a ruled wall inside the cap range), smooth where a claimed facet's
/// centroid rides them, so the difference error at eps = 1e-4 mm is negligible; the whole
/// evaluation recurses inside each placement's local frame, so no direction transform is
/// needed. Beyond an extrude's caps the rim-corner distance is kinked along the seam —
/// there the difference can come out wrong and the cone simply refuses the claim, which
/// leaves the facet free rather than mis-claimed.
fn carrier_normal(e: &fieldsmith_field::expr::Expr, face: Face, p: [f64; 3]) -> Option<[f64; 3]> {
    const EPS: f64 = 1e-4;
    expr::face_carrier_signed(e, face, p)?;
    let mut g = [0.0f64; 3];
    for k in 0..3 {
        let mut pp = p;
        pp[k] += EPS;
        let mut pm = p;
        pm[k] -= EPS;
        g[k] = (expr::face_carrier_signed(e, face, pp)? - expr::face_carrier_signed(e, face, pm)?)
            / (2.0 * EPS);
    }
    let l = (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt();
    (l > 0.0).then_some([g[0] / l, g[1] / l, g[2] / l])
}
