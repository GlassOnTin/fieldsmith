//! STEP (AP203 / AP242) B-rep ingest, goal M10: parse the file with the `step-io`
//! reader (a dependency, not vendored code), tessellate every solid's faces from
//! their stored analytic forms through [`crate::steptess`], and hand the mesh
//! parts to the shared import path. A face whose surface or a boundary curve
//! cannot be consumed exactly (swept surfaces, knotless B-splines, unknown
//! complex instances) is refused by name and fails its whole solid — a silent
//! drop would hand the voxeliser a part with a hole.
//!
//! Units come from the file's own contexts: lengths are scaled to millimetres by
//! the length unit's SI factor, and `CONICAL_SURFACE.semi_angle` is scaled by the
//! plane-angle unit's SI factor (degree-context files write degrees; NIST CTC-01
//! and CTC-04 show both conventions in one corpus).

use std::collections::{HashMap, HashSet};

use crate::steptess::{
    add3, assemble_ring, cross3, dist3, dot3, len3, sample_edge, scale3, sub, tess_face, unit_or,
    EdgePoly, Frame, P2, P3, Ring, RingPart, Surf, VertexPool,
};
use fieldsmith_mesh::export::TriMesh;
use step_io::generated::model::EntityKey;
use step_io::generated::model::{
    Axis2Placement3dRef, Axis2PlacementRef, CartesianPointRef, DirectionRef, StepModel,
};
use step_io::scene::geometry::{CurveKind, Edge, Solid, SurfaceKind};
use step_io::scene::NurbsCurve;

/// A STEP length unit the reader failed to resolve. Kept as the refusal message
/// context rather than silently assuming millimetres.
const NO_LENGTH_UNIT: &str = "the file declares no length unit";

/// One tessellated face's numbers, for CLI reports and verification tests.
#[derive(Clone, Debug)]
pub struct FaceStat {
    /// The face's raw `ADVANCED_FACE.name` (often empty).
    pub name: String,
    /// Which surface form served the face, from `Surf::kind_name`.
    pub surf: String,
    /// Which triangulation path ran (`Surf`'s unbounded grid / structured
    /// grid / earcut refines).
    pub path: &'static str,
    pub tris: u32,
    /// Σ |A| of the output triangles.
    pub mesh_area: f64,
    /// The independent parameter-space area (√g 3-midpoint rule) — the
    /// coverage denominator.
    pub quad_area: f64,
}

/// One solid's tessellation: name, mesh, and the numbers a report cares about.
#[derive(Clone, Debug)]
pub struct StepPart {
    pub name: String,
    pub mesh: TriMesh,
    pub faces: u32,
    pub tris: u32,
    /// The signed volume from the flux sum over all faces (None when any face
    /// was a B-spline — no flux potential there).
    pub volume: Option<f64>,
    /// Σ mesh triangle areas.
    pub mesh_area: f64,
    /// Σ parameter-space face areas.
    pub quad_area: f64,
    /// Per-face numbers, for the cone-coverage rows in verification reports.
    pub face_stats: Vec<FaceStat>,
    /// The chord tolerance this part was tessellated at (mm, sagitta).
    pub chord: f64,
    pub ms: u64,
}

/// A STEP file's tessellation result: the parts plus the provenance a report
/// records (unit, chord, timings, named refusals).
#[derive(Clone, Debug)]
pub struct StepDoc {
    pub parts: Vec<StepPart>,
    /// The file's length unit name, as the reader resolved it.
    pub length_unit: String,
    /// The millimetres-per-file-unit scale applied to all coordinates.
    pub to_mm: f64,
    /// The chord tolerance used for the tessellation (mm, sagitta).
    pub chord: f64,
    pub parse_ms: u64,
    pub tess_ms: u64,
    /// Every whole-solid refusal, named (per the goal: no silent drops).
    pub refusals: Vec<String>,
}

/// The provenance slice of [`StepDoc`] that travels inside the shared import
/// report (the mesh parts are handed to the import path directly).
#[derive(Clone, Debug, serde::Serialize)]
pub struct StepReport {
    pub length_unit: String,
    pub chord: f64,
    pub solids: u32,
    pub refusals: Vec<String>,
    pub parse_ms: u64,
    pub tess_ms: u64,
}

/// Tessellate a STEP file's solids to triangle meshes. `chord` (mm, sagitta)
/// overrides the derived default (`scene diagonal · 3e-4`, clamped to
/// [0.005, 1.0]); a chord tolerance is not optional when the caller passes a
/// non-positive or non-finite value the call fails by name.
pub fn tessellate(bytes: &[u8], chord: Option<f64>) -> Result<StepDoc, String> {
    let t0 = crate::now_ms();
    let (model, _report) =
        step_io::read(bytes).map_err(|e| format!("STEP parse failed: {e}"))?;
    let parse_ms = (crate::now_ms() - t0) as u64;
    let scene = model.scene();

    let units_len = scene
        .units()
        .length
        .ok_or_else(|| NO_LENGTH_UNIT.to_owned())?;
    let to_mm = units_len.to_si * 1000.0;
    let length_unit = units_len.name.clone();
    // Plane-angle measures (CONICAL_SURFACE.semi_angle) follow the file's own
    // plane-angle context: degrees where the file says degrees (CTC-01),
    // radians where it says radians (CTC-04). No angle measure is an error.
    let ang = scene.units().angle.map(|u| u.to_si).unwrap_or(1.0);

    let solids: Vec<_> = scene.all_solids().collect();
    if solids.is_empty() {
        return Err("the file contains no solid geometry".into());
    }
    // Assemblies place solids with transforms this tessellator does not apply;
    // refuse by name rather than import overlapping local placements.
    let occs = model.next_assembly_usage_occurrence_arena.items.len();
    if occs > 0 {
        return Err(format!(
            "the file is an assembly ({occs} placed occurrences); import is single-solid"
        ));
    }

    // name map: solid key → owning product name (empty names fall back below)
    let mut names: HashMap<EntityKey, String> = HashMap::new();
    for def in scene.all_product_definitions() {
        let pname = def.product().map(|p| p.name().to_owned()).unwrap_or_default();
        if pname.is_empty() {
            continue;
        }
        for s in def.solids() {
            names.entry(s.key()).or_insert(pname.clone());
        }
    }

    let chord = match chord {
        Some(c) if c.is_finite() && c > 0.0 => c,
        Some(c) => return Err(format!("chord {c} is not a positive finite tolerance")),
        None => default_chord(&solids, to_mm),
    };

    let mut cache: HashMap<EntityKey, Result<EdgePoly, String>> = HashMap::new();
    let mut parts: Vec<StepPart> = Vec::with_capacity(solids.len());
    let mut refusals: Vec<String> = Vec::new();
    let mut counter: u32 = 0;
    let t1 = crate::now_ms();
    for s in &solids {
        let raw_name = s.name();
        let name = match (!raw_name.trim().is_empty(), names.get(&s.key()).map(String::as_str)) {
            (true, _) => raw_name.to_owned(),
            (false, Some(p)) if !p.trim().is_empty() => p.to_owned(),
            _ => {
                counter += 1;
                format!("part{counter}")
            }
        };
        match tess_solid(&model, s, &name, to_mm, ang, chord, &mut cache) {
            Ok(part) => parts.push(part),
            Err(e) => refusals.push(format!("solid '{name}': {e}")),
        }
    }
    let tess_ms = (crate::now_ms() - t1) as u64;
    if parts.is_empty() {
        let detail = refusals.join("; ");
        return Err(format!(
            "no solid in the file could be tessellated: {detail}"
        ));
    }
    Ok(StepDoc {
        parts,
        length_unit,
        to_mm,
        chord,
        parse_ms,
        tess_ms,
        refusals,
    })
}

/// The derived scene chord tolerance: the diagonal of a whole-scene vertex
/// cover scaled to millimetres, times 3e-4, clamped to [0.005, 1.0] mm.
fn default_chord(solids: &[Solid<'_>], to_mm: f64) -> f64 {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [-f64::INFINITY; 3];
    let mut seen = false;
    let mut cover = |p: [f64; 3]| {
        let p = [p[0] * to_mm, p[1] * to_mm, p[2] * to_mm];
        seen = true;
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    };
    for s in solids {
        for f in s.faces() {
            for b in f.bounds() {
                for (e, _) in b.oriented_edges() {
                    if let Some(p) = e.start().and_then(|v| v.point()) {
                        cover(p.xyz());
                    }
                    if let Some(p) = e.end().and_then(|v| v.point()) {
                        cover(p.xyz());
                    }
                }
            }
        }
    }
    if !seen {
        return 0.1;
    }
    let diag = (hi[0] - lo[0]).hypot(hi[1] - lo[1]).hypot(hi[2] - lo[2]);
    (diag * 3e-4).clamp(0.005, 1.0)
}

/// Resolve one axis placement to (origin, axis, ref_direction). The axis and
/// ref_direction are unit directions; the origin is still in file units.
fn placement(
    model: &StepModel,
    ap_ref: &Axis2Placement3dRef,
) -> Result<(P3, Option<P3>, Option<P3>), String> {
    let Axis2Placement3dRef::Axis2Placement3d(id) = ap_ref;
    let ap = &model.axis2_placement3d_arena.items[id.0];
    use step_io::generated::model::{CartesianPointRef, DirectionRef};
    let CartesianPointRef::CartesianPoint(loc) = &ap.location else {
        return Err("placement origin is a complex point instance".into());
    };
    let coords = &model.cartesian_point_arena.items[loc.0].coordinates;
    if coords.len() != 3 {
        return Err(format!(
            "placement origin has {} coordinates",
            coords.len()
        ));
    }
    let dir = |d: &Option<DirectionRef>| -> Result<Option<P3>, String> {
        match d {
            None => Ok(None),
            Some(DirectionRef::Direction(i)) => {
                let r = &model.direction_arena.items[i.0].direction_ratios;
                if r.len() != 3 {
                    return Err(format!("placement direction has {} ratios", r.len()));
                }
                Ok(Some([r[0], r[1], r[2]]))
            }
            Some(DirectionRef::Complex(_)) => {
                Err("placement direction is a complex entity instance".into())
            }
        }
    };
    let axis = dir(&ap.axis)?;
    let ref_dir = dir(&ap.ref_direction)?;
    Ok((
        [coords[0], coords[1], coords[2]],
        axis,
        ref_dir,
    ))
}

/// Scale a file-space point to millimetres.
fn mm(p: P3, to_mm: f64) -> P3 {
    [p[0] * to_mm, p[1] * to_mm, p[2] * to_mm]
}

/// Build the analytic (or B-spline) surface form of one face. Analytic kinds
/// first — exact parameterisations with flux potentials; anything else falls
/// back to the reader's rational B-spline conversion (`Surface::to_nurbs`
/// resolves complex rational B-splines, then the bounds-aware untrimmed patch
/// covers swept surfaces and unbounded planes) and only then is refused.
fn surf_of(
    model: &StepModel,
    face: &step_io::scene::geometry::Face<'_>,
    to_mm: f64,
    ang: f64,
) -> Result<Surf, String> {
    match face.surface().kind() {
        SurfaceKind::Plane(p) => {
            let (o, axis, rf) = placement(model, &p.position)?;
            let f = Frame::build(
                mm(o, to_mm),
                axis.unwrap_or([0.0, 0.0, 1.0]),
                rf,
            )?;
            Ok(Surf::Plane(f))
        }
        SurfaceKind::Cylindrical(c) => {
            let (o, axis, rf) = placement(model, &c.position)?;
            let Some(axis) = axis else {
                return Err("cylindrical surface without an axis direction".into());
            };
            let f = Frame::build(mm(o, to_mm), axis, rf)?;
            Ok(Surf::Cyl {
                f,
                r: c.radius * to_mm,
            })
        }
        SurfaceKind::Conical(c) => {
            let (o, axis, rf) = placement(model, &c.position)?;
            let Some(axis) = axis else {
                return Err("conical surface without an axis direction".into());
            };
            let f = Frame::build(mm(o, to_mm), axis, rf)?;
            Ok(Surf::Cone {
                f,
                r: c.radius * to_mm,
                k: (c.semi_angle * ang).tan(),
            })
        }
        SurfaceKind::Spherical(c) => {
            let (o, axis, rf) = placement(model, &c.position)?;
            let Some(axis) = axis else {
                return Err("spherical surface without an axis direction".into());
            };
            let f = Frame::build(mm(o, to_mm), axis, rf)?;
            Ok(Surf::Sphere {
                f,
                r: c.radius * to_mm,
            })
        }
        SurfaceKind::Toroidal(c) => {
            let (o, axis, rf) = placement(model, &c.position)?;
            let Some(axis) = axis else {
                return Err("toroidal surface without an axis direction".into());
            };
            let f = Frame::build(mm(o, to_mm), axis, rf)?;
            Ok(Surf::Torus {
                f,
                big_r: c.major_radius * to_mm,
                r: c.minor_radius * to_mm,
            })
        }
        SurfaceKind::LinearExtrusion(_) | SurfaceKind::Revolution(_) | SurfaceKind::BSpline(_)
        | SurfaceKind::BSplineWithKnots(_) | SurfaceKind::QuasiUniform(_)
        | SurfaceKind::Uniform(_) | SurfaceKind::Bezier(_) | SurfaceKind::Other(_) => {
            if let Some(n) = face.surface().to_nurbs() {
                return Ok(Surf::Nurbs(Box::new(scale_nurbs(n, to_mm))));
            }
            if let Some(n) = face.to_nurbs() {
                return Ok(Surf::Nurbs(Box::new(scale_nurbs(n, to_mm))));
            }
            Err(format!(
                "the {} surface has no tessellatable form",
                surf_label(face.surface().kind())
            ))
        }
    }
}

/// The reader's name for the surface kind that was refused, for the
/// named-refusal report.
fn surf_label(k: SurfaceKind<'_>) -> String {
    match k {
        SurfaceKind::Plane(_) => "PLANE".into(),
        SurfaceKind::Cylindrical(_) => "CYLINDRICAL_SURFACE".into(),
        SurfaceKind::Conical(_) => "CONICAL_SURFACE".into(),
        SurfaceKind::Spherical(_) => "SPHERICAL_SURFACE".into(),
        SurfaceKind::Toroidal(_) => "TOROIDAL_SURFACE".into(),
        SurfaceKind::BSpline(_) => "B_SPLINE_SURFACE (without knot data)".into(),
        SurfaceKind::BSplineWithKnots(_) => "B_SPLINE_SURFACE_WITH_KNOTS".into(),
        SurfaceKind::LinearExtrusion(_) => "SURFACE_OF_LINEAR_EXTRUSION".into(),
        SurfaceKind::Revolution(_) => "SURFACE_OF_REVOLUTION".into(),
        SurfaceKind::QuasiUniform(_) => "QUASI_UNIFORM_SURFACE".into(),
        SurfaceKind::Uniform(_) => "UNIFORM_SURFACE".into(),
        SurfaceKind::Bezier(_) => "BEZIER_SURFACE".into(),
        SurfaceKind::Other(kw) => kw.to_string(),
    }
}

/// Scale a B-spline surface's control points from file units to millimetres
/// (weights untouched — the projective weight only re-normalises).
fn scale_nurbs(
    mut n: step_io::scene::NurbsSurface,
    to_mm: f64,
) -> step_io::scene::NurbsSurface {
    if to_mm != 1.0 {
        for row in n.control_points.iter_mut() {
            for p in row.iter_mut() {
                *p = mm(*p, to_mm);
            }
        }
    }
    n
}

/// The reader's `Edge::to_nurbs` refuses a circle/ellipse whose placement frame
/// degenerates — a `ref_direction` parallel to the axis reduces the projected
/// x reference to a zero vector (48 such circles, fillets on ±X-axis bores,
/// in NIST CTC-04). The frame is still well-defined: STEP fixes z, and x is
/// any perpendicular direction — a rotated x only shifts the arc's parameter
/// origin, the swept point set is unchanged. This resolves the frame with the
/// same deterministic fallback as the surfaces use ([`Frame::build`]) and
/// rebuilds the same degree-2 rational arc from the vertex angles. `None` for
/// every other curve kind (the caller keeps its named refusal) and for 2D
/// placements or unresolved vertices, where the reader's refusal was correct.
fn conic_edge_fallback(model: &StepModel, e: &Edge<'_>) -> Option<NurbsCurve> {
    use std::f64::consts::TAU;
    let (p0, p1) = (
        e.start().and_then(|v| v.point()).map(|p| p.xyz()),
        e.end().and_then(|v| v.point()).map(|p| p.xyz()),
    );
    let (Some(p0), Some(p1)) = (p0, p1) else {
        return None;
    };
    let sense = e.same_sense();
    let plc_ab = match e.curve().kind() {
        CurveKind::Circle(c) => (&c.position, (c.radius, c.radius)),
        CurveKind::Ellipse(el) => (&el.position, (el.semi_axis_1, el.semi_axis_2)),
        _ => return None,
    };
    let (plc, ab) = plc_ab;
    if !(ab.0 > 0.0 && ab.1 > 0.0) {
        return None;
    }
    let frame = match plc {
        Axis2PlacementRef::Axis2Placement3d(id) => {
            let p = model.axis2_placement3d_arena.get(id.0);
            let loc: P3 = match &p.location {
                CartesianPointRef::CartesianPoint(cid) => {
                    let c = &model.cartesian_point_arena.get(cid.0).coordinates;
                    [
                        c.first().copied().unwrap_or(0.0),
                        c.get(1).copied().unwrap_or(0.0),
                        c.get(2).copied().unwrap_or(0.0),
                    ]
                }
                _ => return None,
            };
            let ratios = |d: &DirectionRef| -> Option<P3> {
                match d {
                    DirectionRef::Direction(did) => {
                        let v = &model.direction_arena.get(did.0).direction_ratios;
                        Some([
                            v.first().copied().unwrap_or(0.0),
                            v.get(1).copied().unwrap_or(0.0),
                            v.get(2).copied().unwrap_or(0.0),
                        ])
                    }
                    DirectionRef::Complex(_) => None,
                }
            };
            let z = match &p.axis {
                Some(a) => ratios(a)?,
                None => [0.0, 0.0, 1.0],
            };
            let rd = match &p.ref_direction {
                Some(d) => Some(ratios(d)?),
                None => None,
            };
            Frame::build(loc, z, rd).ok()?
        }
        Axis2PlacementRef::Axis2Placement2d(_) => return None,
    };
    let (center, x, y) = (frame.o, frame.x, frame.y);
    // the conic's angle at a point, in the frame's own (x, y)
    let angle = |p: P3| {
        let d = [p[0] - center[0], p[1] - center[1], p[2] - center[2]];
        ((d[0] * y[0] + d[1] * y[1] + d[2] * y[2]) / ab.1)
            .atan2((d[0] * x[0] + d[1] * x[1] + d[2] * x[2]) / ab.0)
    };
    let theta1 = angle(p0);
    let mut theta2 = angle(p1);
    if !(theta1.is_finite() && theta2.is_finite()) {
        return None;
    }
    // a closed edge (coincident vertices) sweeps the whole conic in the
    // curve's parameter direction; an open edge sweeps start → end
    if sense {
        while theta2 <= theta1 {
            theta2 += TAU;
        }
    } else {
        while theta2 >= theta1 {
            theta2 -= TAU;
        }
    }
    let sweep = theta2 - theta1;
    let narcs = ((sweep.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize).max(1);
    let dtheta = sweep / narcs as f64;
    let w1 = (dtheta.abs() / 2.0).cos();
    let world = |lx: f64, ly: f64| -> P3 {
        [
            center[0] + x[0] * (lx * ab.0) + y[0] * (ly * ab.1),
            center[1] + x[1] * (lx * ab.0) + y[1] * (ly * ab.1),
            center[2] + x[2] * (lx * ab.0) + y[2] * (ly * ab.1),
        ]
    };
    let mut control_points = vec![world(theta1.cos(), theta1.sin())];
    let mut weights = vec![1.0];
    for k in 0..narcs {
        let mid = theta1 + dtheta * (k as f64 + 0.5);
        control_points.push(world(mid.cos() / w1, mid.sin() / w1));
        weights.push(w1);
        let end = theta1 + dtheta * (k as f64 + 1.0);
        control_points.push(world(end.cos(), end.sin()));
        weights.push(1.0);
    }
    let mut knots = vec![0.0, 0.0, 0.0];
    for i in 1..narcs {
        knots.push(i as f64);
        knots.push(i as f64);
    }
    let n = narcs as f64;
    knots.extend([n, n, n]);
    Some(NurbsCurve {
        degree: 2,
        control_points,
        weights,
        knots,
    })
}

// ---------------------------------------------------------------------------
// shared-boundary master polylines
// ---------------------------------------------------------------------------

/// The conic geometry an edge lies on, in millimetres, as written by its own
/// entity (axis not sign-canonicalised — [`GeoKey`] does that for grouping).
#[derive(Clone, Copy, Debug)]
enum Geo {
    /// (origin, normalised axis, radius)
    Circle(P3, P3, f64),
    /// (canonical frame from the entity's placement, semi axes 1 and 2)
    Ellipse(Frame, f64, f64),
}

fn quant(p: P3) -> [u64; 3] {
    [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()]
}

/// Deterministic axis sign: the largest-|component| (lowest index wins ties)
/// is made positive, so duplicates of one geometric circle written with either
/// axis sense agree.
fn sign_fix(v: P3) -> P3 {
    let d = if v[0].abs() >= v[1].abs() && v[0].abs() >= v[2].abs() {
        0
    } else if v[1].abs() >= v[2].abs() {
        1
    } else {
        2
    };
    if v[d] < 0.0 {
        [-v[0], -v[1], -v[2]]
    } else {
        v
    }
}

/// The geometric identity an edge's curve groups by (exact bits of the
/// millimetre geometry; the axis and the ellipse's own x direction are
/// sign-canonicalised so ± written duplicates group, a genuinely different
/// frame does not group and keeps its per-entity sampling).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum GeoKey {
    Circle { c: [u64; 3], r: u64, ax: [u64; 3] },
    Ellipse { c: [u64; 3], a: u64, b: u64, ax: [u64; 3], ex: [u64; 3] },
}

impl GeoKey {
    fn of(geo: &Geo) -> GeoKey {
        match *geo {
            Geo::Circle(c, ax, r) => GeoKey::Circle {
                c: quant(c),
                r: r.to_bits(),
                ax: quant(sign_fix(ax)),
            },
            Geo::Ellipse(f, a, b) => GeoKey::Ellipse {
                c: quant(f.o),
                a: a.to_bits(),
                b: b.to_bits(),
                ax: quant(sign_fix(f.z)),
                ex: quant(sign_fix(f.x)),
            },
        }
    }
}

/// One registered conic edge: its vertex endpoints (mm), closedness from the
/// topological vertices, the edge's own entity axis, and the shared geometry.
#[derive(Clone, Copy, Debug)]
struct EdgeReg {
    start: P3,
    end: P3,
    /// start vertex is the end vertex (a closed entity).
    closed: bool,
    /// the edge's start → end direction agrees with its curve's parameter
    /// direction (`EDGE_CURVE.same_sense`).
    same_sense: bool,
    /// the edge's own entity axis, normalised (the master's frame is
    /// sign-canonicalised, so composing the two gives the sweep's canonical
    /// direction for any written duplicate).
    axis_file: P3,
    geo: Geo,
}

/// Resolve a 3D axis placement to (origin, axis, reference direction), file units.
fn placement_parts(model: &StepModel, plc: &Axis2PlacementRef) -> Option<(P3, P3, Option<P3>)> {
    let Axis2PlacementRef::Axis2Placement3d(id) = plc else {
        return None;
    };
    let p = model.axis2_placement3d_arena.get(id.0);
    let ratios = |d: &DirectionRef| -> Option<P3> {
        match d {
            DirectionRef::Direction(did) => {
                let v = &model.direction_arena.get(did.0).direction_ratios;
                Some([
                    v.first().copied().unwrap_or(0.0),
                    v.get(1).copied().unwrap_or(0.0),
                    v.get(2).copied().unwrap_or(0.0),
                ])
            }
            DirectionRef::Complex(_) => None,
        }
    };
    let loc = match &p.location {
        CartesianPointRef::CartesianPoint(cid) => {
            let c = &model.cartesian_point_arena.get(cid.0).coordinates;
            [
                c.first().copied().unwrap_or(0.0),
                c.get(1).copied().unwrap_or(0.0),
                c.get(2).copied().unwrap_or(0.0),
            ]
        }
        _ => return None,
    };
    let z = match &p.axis {
        Some(a) => ratios(a)?,
        None => [0.0, 0.0, 1.0],
    };
    let rd = match &p.ref_direction {
        Some(d) => Some(ratios(d)?),
        None => None,
    };
    Some((loc, z, rd))
}

/// The registration half of [`register_conic_edge`]: the geometric forms this
/// tessellator masters. `None` leaves the edge on the per-entity path.
fn conic_geometry(model: &StepModel, e: &Edge<'_>, to_mm: f64) -> Option<Geo> {
    match e.curve().kind() {
        CurveKind::Circle(c) => {
            let (loc, z, _rd) = placement_parts(model, &c.position)?;
            let center = mm(loc, to_mm);
            let radius = c.radius * to_mm;
            if radius <= 0.0 || len3(z) < 1e-12 {
                return None;
            }
            Some(Geo::Circle(center, unit_or(z, [0.0, 0.0, 0.0]), radius))
        }
        CurveKind::Ellipse(el) => {
            let (loc, z, rd) = placement_parts(model, &el.position)?;
            let a = el.semi_axis_1 * to_mm;
            let b = el.semi_axis_2 * to_mm;
            if !(a > 0.0 && b > 0.0) {
                return None;
            }
            // the entity's own frame: x is the semi-axis-1 direction, y = z × x
            let f = Frame::build(mm(loc, to_mm), z, rd).ok()?;
            Some(Geo::Ellipse(f, a, b))
        }
        _ => None,
    }
}

fn conic_kind_name(geo: &Geo) -> &'static str {
    match geo {
        Geo::Circle(..) => "CIRCLE",
        Geo::Ellipse(..) => "ELLIPSE",
    }
}

/// Register a conic edge for shared-boundary master sampling: endpoints (mm),
/// topological closedness, the entity's own axis, and the shared geometry.
/// `None` leaves the edge on the per-entity sampling path (unresolved
/// vertices, 2D placements, non-conic curves).
fn register_conic_edge(model: &StepModel, e: &Edge<'_>, to_mm: f64) -> Option<EdgeReg> {
    let (Some(vs), Some(ve)) = (e.start(), e.end()) else {
        return None;
    };
    let (Some(ps), Some(pe)) = (vs.point(), ve.point()) else {
        return None;
    };
    let geo = conic_geometry(model, e, to_mm)?;
    Some(EdgeReg {
        start: mm(ps.xyz(), to_mm),
        end: mm(pe.xyz(), to_mm),
        closed: vs.key() == ve.key(),
        same_sense: e.same_sense(),
        axis_file: match geo {
            Geo::Circle(_, ax, _) => ax,
            Geo::Ellipse(f, _, _) => f.z,
        },
        geo,
    })
}

/// One constraint-aware master polyline over a geometric circle or ellipse:
/// every member edge's vertices pin the sampling, the samples run ascending
/// in the canonical angle, and each member edge slices the span it trims.
/// Overlapping slices (a full-circle trim on one face, quarter arcs on its
/// neighbour) therefore share exact points, which the vertex pool welds.
struct ConicMaster {
    center: P3,
    e1: P3,
    e2: P3,
    n: P3,
    a: f64,
    b: f64,
    /// samples ascending in the canonical angle; `ang[0]` is `ang0` and the
    /// wrap arc may run past `ang0 + 2π` (ascending, never re-normalised).
    pts: Vec<P3>,
    ang: Vec<f64>,
    ang0: f64,
    /// constraint angles ascending (each member edge's endpoints) and their
    /// sample indices; `cons[k] == ang[cons_idx[k]]` exactly.
    cons: Vec<f64>,
    cons_idx: Vec<usize>,
}

impl ConicMaster {
    /// Map a query angle into the master's domain `[ang0, ang0 + 2π)`.
    fn domain(&self, th: f64) -> f64 {
        self.ang0 + (th - self.ang0).rem_euclid(std::f64::consts::TAU)
    }

    /// The canonical angle of a point: the master's parameters place the full
    /// conic, so the angle comes from the canonical frame alone.
    fn angle_of(&self, p: P3) -> f64 {
        let d = sub(p, self.center);
        (dot3(d, self.e2) / self.b).atan2(dot3(d, self.e1) / self.a)
    }

    fn at(&self, th: f64) -> P3 {
        let (s, c) = th.sin_cos();
        add3(
            self.center,
            add3(scale3(self.e1, self.a * c), scale3(self.e2, self.b * s)),
        )
    }

    /// Build the master for one geometric group (≥ 2 registered edges sharing
    /// one [`GeoKey`]). The representative's geometry supplies the frame —
    /// duplicates share it by the key's construction — and every member's
    /// vertex endpoints pin the sampling.
    fn build(reps: &[EdgeReg], chord: f64) -> Result<ConicMaster, String> {
        let rep = &reps[0];
        let (center, e1, e2, n, a, b) = match rep.geo {
            Geo::Circle(_, axis, r) => {
                // canonical frame: sign-fixed axis, world-derived in-plane basis
                let n = sign_fix(axis);
                let world = if dot3([1.0, 0.0, 0.0], n).abs() < 0.9 {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 1.0, 0.0]
                };
                let e1 = unit_or(cross3(n, world), [0.0, 0.0, 0.0]);
                if len3(e1) < 0.9 {
                    return Err("circle placement axis is degenerate".to_owned());
                }
                let Geo::Circle(center, _, _) = rep.geo else {
                    unreachable!()
                };
                (center, e1, cross3(n, e1), n, r, r)
            }
            Geo::Ellipse(f, a, b) => {
                let n = sign_fix(f.z);
                let e1 = sign_fix(f.x);
                (f.o, e1, cross3(n, e1), n, a, b)
            }
        };
        let mut m = ConicMaster {
            center,
            e1,
            e2,
            n,
            a,
            b,
            pts: Vec::new(),
            ang: Vec::new(),
            ang0: 0.0,
            cons: Vec::new(),
            cons_idx: Vec::new(),
        };
        // constraints: every member vertex's canonical angle, ascending
        let mut ths: Vec<f64> = reps
            .iter()
            .flat_map(|r| [m.angle_of(r.start), m.angle_of(r.end)])
            .collect();
        if ths.iter().any(|t| !t.is_finite()) {
            return Err("a trim vertex sits at the conic's centre".to_owned());
        }
        ths.sort_by(|x, y| x.partial_cmp(y).unwrap());
        ths.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
        let ang0 = ths[0];
        m.ang0 = ang0;
        m.cons = ths;
        // arcs between consecutive constraints (the last wraps to ang0 + 2π;
        // a single constraint sweeps the full circle). Each arc divides into
        // ceil(span / Δmax) equal steps — the sagitta bound with the same
        // per-sample turn cap sample_edge's adaptive refinement enforces —
        // and only its interior points are appended: slices of overlapping
        // edges share whole arcs, so only whole arcs can ever be shared.
        let rmin = match rep.geo {
            Geo::Circle(_, _, r) => r,
            Geo::Ellipse(_, a, b) => (a.min(b) * a.min(b)) / a.max(b),
        };
        let dmax = (2.0 * (1.0 - (chord / rmin).clamp(0.0, 1.0)).acos()).min(0.35);
        let k = m.cons.len();
        for c in 0..k {
            m.cons_idx.push(m.pts.len());
            let cur = m.cons[c];
            let nxt = if c + 1 == k { ang0 + std::f64::consts::TAU } else { m.cons[c + 1] };
            let span = nxt - cur;
            let nsteps = ((span / dmax).ceil() as usize).max(1);
            let dth = span / nsteps as f64;
            for i in 0..nsteps {
                let th = cur + dth * i as f64;
                m.pts.push(m.at(th));
                m.ang.push(th);
            }
        }
        Ok(m)
    }

    fn slice(&self, reg: &EdgeReg) -> Result<EdgePoly, String> {
        let qs = self.domain(self.angle_of(reg.start));
        let qe = self.domain(self.angle_of(reg.end));
        // the edge sweeps its curve's parameter direction (same_sense) around
        // its own entity axis; the master's frame is sign-canonicalised, so
        // the two compose by the axis hands' agreement
        let hand = if dot3(self.n, reg.axis_file) >= 0.0 { 1.0 } else { -1.0 };
        let sgn = (if reg.same_sense { 1.0 } else { -1.0 }) * hand;
        let cons_of = |q: f64| self.cons.iter().position(|&c| (c - q).abs() <= 1e-9);
        // constraints index their own array; slices walk the samples, through
        // the recorded sample index of each constraint
        let is = self.cons_idx[cons_of(qs)
            .ok_or_else(|| format!("trim start angle {qs:.9} is not a master constraint"))?];
        let ie = self.cons_idx[cons_of(qe)
            .ok_or_else(|| format!("trim end angle {qe:.9} is not a master constraint"))?];
        let m = self.pts.len();
        let closed = reg.closed
            || dist3(reg.start, reg.end) <= 1e-9 * (1.0 + len3(reg.start).max(len3(reg.end)));
        let mut out: Vec<P3> = Vec::new();
        let step = |i: usize| -> usize {
            if sgn > 0.0 {
                (i + 1) % m
            } else {
                (i + m - 1) % m
            }
        };
        if closed {
            let mut i = is;
            for _ in 0..m {
                out.push(self.pts[i]);
                i = step(i);
            }
            out.push(self.pts[is]);
        } else {
            let mut i = is;
            let mut steps = 0usize;
            loop {
                out.push(self.pts[i]);
                if i == ie {
                    break;
                }
                i = step(i);
                steps += 1;
                if steps > m {
                    return Err("the trim end angle was never reached".to_owned());
                }
            }
        }
        // the file's vertices pin the ends (as on every sampled polyline):
        // the master's samples sit on the ideal conic, the vertices carry the
        // authoring slop, and rings meet at the vertices — in the edge's own
        // start → end direction
        if out.len() >= 2 {
            out[0] = reg.start;
            let last = out.len() - 1;
            out[last] = reg.end;
        } else if out.len() == 1 {
            out[0] = reg.start;
        }
        Ok(EdgePoly {
            pts: out,
            entity_closed: closed,
        })
    }
}
/// edge cache (one sample per `EDGE_CURVE`), enter the same `VertexPool` (the
/// quantised keys weld shared corners across faces), and any face refusal fails
/// the whole solid by name.
fn tess_solid(
    model: &StepModel,
    solid: &Solid<'_>,
    name: &str,
    to_mm: f64,
    ang: f64,
    chord: f64,
    cache: &mut HashMap<EntityKey, Result<EdgePoly, String>>,
) -> Result<StepPart, String> {
    let t0 = crate::now_ms();
    // two tolerances: the ring walk joins on topological vertices, which real
    // files share exactly, so that check stays tight; an inversion accepts up
    // to the tessellation chord because the file's own trimmed curves (3D
    // intersection curves, mostly B-splines) sit off their analytic surfaces
    // by the authoring system's curve tolerance, which is beyond the walk
    // tolerance but far inside the fidelity the tessellation names
    let join_tol = (chord * 1e-3).clamp(1e-9, 0.001);
    let inv_tol = chord.max(1e-6);
    let mut faces_out: Vec<(Surf, bool, String, Vec<Ring>)> = Vec::new();
    let mut pts_all: Vec<P3> = Vec::new();

    let mut shells: Vec<Vec<step_io::scene::geometry::Face<'_>>> = Vec::new();
    shells.push(solid.faces().collect());
    for void in solid.voids() {
        shells.push(void.into_iter().collect());
    }

    // pass 0: shared-boundary masters for conic edges. Two faces bounding the
    // same geometric circle or ellipse through *different* EDGE_CURVE entities
    // (one side trims the circle into arcs the other crosses whole — a legal
    // topological T-vertex) each sampled their own polyline before: the samples
    // interleaved at sub-millimetre offsets, nothing welded, and the trims
    // opened. One constraint-aware master polyline per geometric curve fixes
    // every decomposition at once — all of the group's vertices pin the
    // sampling, each edge slices the span it trims, and overlapping slices
    // share exact points, which the vertex pool welds. Single-edge curves keep
    // their per-entity sampling.
    let mut seen_conic: HashSet<EntityKey> = HashSet::new();
    let mut regs: Vec<(EntityKey, EdgeReg)> = Vec::new();
    let mut groups: HashMap<GeoKey, Vec<usize>> = HashMap::new();
    for faces in &shells {
        for face in faces {
            for b in face.bounds() {
                for (e, _) in b.oriented_edges() {
                    if !seen_conic.insert(e.key()) {
                        continue;
                    }
                    if let Some(reg) = register_conic_edge(model, &e, to_mm) {
                        groups.entry(GeoKey::of(&reg.geo)).or_default().push(regs.len());
                        regs.push((e.key(), reg));
                    }
                }
            }
        }
    }
    let mut masters: HashMap<EntityKey, Result<EdgePoly, String>> = HashMap::new();
    for members in groups.into_values() {
        if members.len() < 2 {
            continue;
        }
        let reps: Vec<EdgeReg> = members.iter().map(|&i| regs[i].1).collect();
        match ConicMaster::build(&reps, chord) {
            Ok(master) => {
                for (i, reg) in reps.iter().enumerate() {
                    let poly = master.slice(reg).map_err(|e| {
                        format!("shared {} trim: {e}", conic_kind_name(&reg.geo))
                    });
                    masters.insert(regs[members[i]].0, poly);
                }
            }
            Err(e) => {
                for &i in &members {
                    masters.insert(regs[i].0, Err(e.clone()));
                }
            }
        }
    }
    // the edge cache serves master slices like any sampled polyline: one slice
    // per entity, both seam occurrences included
    for (k, v) in &masters {
        cache.insert(*k, v.clone());
    }

    // pass 1: resolve bounds to rings (also collects the pool's extent)
    let mut counted: HashMap<EntityKey, u32> = HashMap::new();
    for faces in &shells {
        for face in faces {
            let surf = surf_of(model, face, to_mm, ang)?;
            let mut rings: Vec<Ring> = Vec::new();
            let mut occs: HashMap<EntityKey, u32> = HashMap::new();
            for b in face.bounds() {
                let mut pairs: Vec<(step_io::scene::geometry::Edge<'_>, bool)> =
                    b.oriented_edges().collect();
                if pairs.is_empty() {
                    // vertex-loop bound: a degenerate marker (a cone apex, a
                    // sphere pole) whose point lies where the window closes
                    // off. No walk is possible; the face's other bound carries
                    // the rim and the structured window widens to the singular
                    // parameter edge from a degenerate v hull (CTC-05 drill
                    // points). A face with no other real bound then fails
                    // downstream with "no whole-surface form", which is the
                    // correct refusal for a point-window face.
                    continue;
                }
                if !b.orientation() {
                    // the caller folds a face bound's orientation: reverse the
                    // walk order and flip every per-edge direction flag
                    pairs.reverse();
                    for p in pairs.iter_mut() {
                        p.1 = !p.1;
                    }
                }
                let mut parts: Vec<RingPart> = Vec::with_capacity(pairs.len());
                for (e, fwd) in pairs {
                    let key = e.key();
                    *occs.entry(key).or_insert(0) += 1;
                    let occ = occs[&key] - 1;
                    *counted.entry(key).or_insert(0) += 1;
                    let poly = match cache.get(&key) {
                        Some(Ok(p)) => p.clone(),
                        Some(Err(ev)) => return Err(ev.clone()),
                        None => {
                            // the reader's curve lives in file units — sample in
                            // millimetres so the ring matches the mm-scaled surfaces
                            let built = match e.to_nurbs() {
                                Some(mut c) => {
                                    for cp in c.control_points.iter_mut() {
                                        *cp = mm(*cp, to_mm);
                                    }
                                    sample_edge(&c, chord)
                                }
                                None => match conic_edge_fallback(model, &e) {
                                    Some(mut c) => {
                                        for cp in c.control_points.iter_mut() {
                                            *cp = mm(*cp, to_mm);
                                        }
                                        sample_edge(&c, chord)
                                    }
                                    None => Err(format!(
                                        "edge curve ({}) has no B-spline form",
                                        edge_label(e.curve().kind())
                                    )),
                                },
                            }
                            .map(|mut poly| {
                                // An edge's bounds ARE its two vertices; a
                                // curve whose written endpoint sits off its
                                // vertex is the authoring system's own slop
                                // (NIST CTC-05: 2.6e-4 mm on a B-spline
                                // intersection curve). Pinning both ends to
                                // the vertices keeps every face of the shared
                                // edge on the same polyline and the walk's
                                // continuity check meaningful — the slop
                                // rides inside the tessellation chord named
                                // below.
                                let (vs, ve) = (
                                    e.start().and_then(|v| v.point()),
                                    e.end().and_then(|v| v.point()),
                                );
                                if let (Some(s), Some(t)) = (vs, ve) {
                                    if let Some(p) = poly.pts.first_mut() {
                                        *p = mm(s.xyz(), to_mm);
                                    }
                                    if let Some(p) = poly.pts.last_mut() {
                                        *p = mm(t.xyz(), to_mm);
                                    }
                                }
                                poly
                            });
                            cache.insert(key, built.clone());
                            built?
                        }
                    };
                    pts_all.extend_from_slice(&poly.pts);
                    parts.push(RingPart { poly, fwd, occ });
                }
                rings.push(assemble_ring(&surf, &parts, b.is_outer(), join_tol, inv_tol)?);
            }
            // STEP lets a face's outer bound be left unlabelled (every bound a
            // FACE_BOUND — NIST CTC-01's plate faces); the outer loop is then
            // a topological fact, not a flag. In parameter space the outer
            // bound contains the others: the largest-area polygon whose
            // interior every other ring's samples wind inside. A
            // single-bound face promotes outright.
            if !rings.iter().any(|r| r.is_outer) {
                promote_outer(&surf, &mut rings)?;
            }
            faces_out.push((surf, face.same_sense(), face.name().to_owned(), rings));
        }
    }
    // manifold count: every EDGE_CURVE borders exactly two face occurrences
    for (key, c) in &counted {
        if *c != 2 {
            return Err(format!(
                "edge entity #{:?} is used {c} times (manifold shells use 2)",
                *key
            ));
        }
    }
    if pts_all.is_empty() {
        return Err("the solid has no tessellatable faces".into());
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [-f64::INFINITY; 3];
    for p in &pts_all {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    let diag = (hi[0] - lo[0]).hypot(hi[1] - lo[1]).hypot(hi[2] - lo[2]);

    // pass 2: triangulate
    let mut pool = VertexPool::new(diag);
    let mut idx: Vec<[u32; 3]> = Vec::new();
    let mut mesh_area = 0.0;
    let mut quad_area = 0.0;
    let mut volume = Some(0.0);
    let mut tris_total = 0u32;
    let mut tri_face_start: Vec<usize> = Vec::new();
    let mut face_stats: Vec<FaceStat> = Vec::new();
    for (fi, (surf, same_sense, fname, rings)) in faces_out.iter().enumerate() {
        let t = tess_face(surf, *same_sense, rings, &mut pool, chord, join_tol)
            .map_err(|e| format!("face {fi} ({}): {e}", surf.kind_name()))?;
        tri_face_start.push(idx.len());
        for t3 in &t.tris {
            idx.push(*t3);
        }
        mesh_area += t.mesh_area;
        quad_area += t.quad_area;
        tris_total += t.tris.len() as u32;
        face_stats.push(FaceStat {
            name: fname.clone(),
            surf: surf.kind_name().to_owned(),
            path: t.path,
            tris: t.tris.len() as u32,
            mesh_area: t.mesh_area,
            quad_area: t.quad_area,
        });
        if let Some(v) = t.volume_face {
            volume = volume.map(|acc| acc + v);
        } else {
            volume = None;
        }
    }
    // Global triangle orientation. Each tessellation path emits raw windings
    // that agree with the surface's natural normal (earcut normalises its
    // output; the structured and unbounded grids walk ascending (u, v)), but
    // whether that is the face's material side is the file's `same_sense`
    // per face — and half-window faces give no independent check: the old
    // per-triangle flip through surf.invert(centroid) silently unwrapped
    // NIST CTC-04's half-window cones and torus patches onto a parameter
    // branch a period away, negating the natural normal there and
    // mis-flipping triangles (PrusaSlicer read the result as 9412 open
    // edges; an edge whose two faces traverse it identically pairs as open).
    // Decide orientation once for the whole welded solid: propagate winding
    // across shared edges (each edge must be traversed once per direction),
    // then set the global side by the analytic flux volume's sign — the two
    // volume conventions must agree, and the solid's, when both do.
    orient_solid(&pool.verts, &mut idx, volume);
    Ok(StepPart {
        name: name.to_owned(),
        mesh: TriMesh {
            verts: pool.verts.clone(),
            tris: idx,
        },
        faces: faces_out.len() as u32,
        tris: tris_total,
        volume,
        mesh_area,
        quad_area,
        face_stats,
        chord,
        ms: (crate::now_ms() - t0) as u64,
    })
}

/// Orient the welded solid's triangles consistently and outward. Winding
/// propagates across shared edges (a manifold edge must be traversed once per
/// direction; an edge whose two tris traverse it identically is what slicers
/// pair as open), then the global side is set by the analytic flux volume's
/// sign — the tessellated divergence and the B-rep's analytic volume must
/// agree in sign, not just magnitude. A flux of None (a face a B-spline
/// refusal left without a potential) keeps the propagated orientation.
fn orient_solid(verts: &[P3], tris: &mut [[u32; 3]], flux_volume: Option<f64>) {
    // undirected edge -> the (tri, slot) pairs using it
    let mut by_edge: HashMap<[u32; 2], Vec<(u32, u8)>> = HashMap::new();
    for (ti, t) in tris.iter().enumerate() {
        for k in 0..3 {
            by_edge
                .entry([t[k].min(t[(k + 1) % 3]), t[k].max(t[(k + 1) % 3])])
                .or_default()
                .push((ti as u32, k as u8));
        }
    }
    // Propagation: any tri already given an orientation fixes every tri it
    // shares a directed edge with — the partner must traverse the edge the
    // opposite way, or it is reversed now. Reachability is what a BFS gives.
    let mut visited = vec![false; tris.len()];
    let mut stack: Vec<u32> = Vec::new();
    for seed in 0..tris.len() as u32 {
        if visited[seed as usize] {
            continue;
        }
        visited[seed as usize] = true;
        stack.push(seed);
        while let Some(t) = stack.pop() {
            let ta = t as usize;
            for k in 0..3u8 {
                let (a, b) = (
                    tris[ta][k as usize],
                    tris[ta][((k as usize) + 1) % 3],
                );
                let key = [a.min(b), a.max(b)];
                for &(p, s) in by_edge.get(&key).into_iter().flatten() {
                    let pi = p as usize;
                    if pi == ta {
                        continue;
                    }
                    if !visited[pi] {
                        visited[pi] = true;
                        stack.push(p);
                        let (pa, pb) = (
                            tris[pi][s as usize],
                            tris[pi][(s as usize + 1) % 3],
                        );
                        if (pa, pb) == (a, b) {
                            tris[pi].reverse();
                        }
                    }
                }
            }
        }
    }
    // Parity check: a solid whose own topology cannot satisfy the
    // one-per-direction rule leaves residual conflicts no orientation
    // assignment removes. Re-run the conflict sweep a bounded number of
    // times so a late-discovered flip cannot strand one behind it.
    for _ in 0..4 {
        let mut flipped = 0usize;
        for ti in 0..tris.len() as u32 {
            for k in 0..3u8 {
                let (a, b) = (
                    tris[ti as usize][k as usize],
                    tris[ti as usize][(k as usize + 1) % 3],
                );
                let key = [a.min(b), a.max(b)];
                for &(p, s) in by_edge.get(&key).into_iter().flatten() {
                    let pi = p as usize;
                    if pi == ti as usize {
                        continue;
                    }
                    if (tris[pi][s as usize], tris[pi][(s as usize + 1) % 3]) == (a, b) {
                        tris[pi].reverse();
                        flipped += 1;
                    }
                }
            }
        }
        if flipped == 0 {
            break;
        }
    }
    // Sign: divergence over the propagated orientation, matched against the
    // analytic flux. Zero divergence (nothing enclosed) keeps the mesh as is.
    let mut div = 0.0f64;
    for t in tris.iter() {
        let (a, b, c) = (verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]);
        div += (a[0] * (b[1] * c[2] - c[1] * b[2]) - a[1] * (b[0] * c[2] - c[0] * b[2])
            + a[2] * (b[0] * c[1] - c[0] * b[1]))
            / 6.0;
    }
    if let Some(v) = flux_volume {
        if v != 0.0 && div * v < 0.0 {
            for t in tris.iter_mut() {
                t.reverse();
            }
        }
    }
}

/// Label a face's outer bound when the file left every bound a `FACE_BOUND`
/// (ISO 10303-42 permits the outer bound to be omitted; NIST CTC-01's plate
/// faces do). In parameter space the outer bound contains the others: the
/// largest |u,v| polygon whose interior every other ring's samples wind
/// inside. A single-bound face promotes outright; anything whose test fails
/// is refused by name — two equal rims of a full-period band hold no outer
/// in this shape and that refusal names the count.
fn promote_outer(surf: &Surf, rings: &mut Vec<Ring>) -> Result<(), String> {
    if rings.len() <= 1 {
        if let Some(r) = rings.first_mut() {
            r.is_outer = true;
        }
        return Ok(());
    }
    let area = |uv: &[P2]| -> f64 {
        uv.windows(2)
            .map(|w| w[0][0] * w[1][1] - w[1][0] * w[0][1])
            .sum::<f64>()
            .abs()
            / 2.0
    };
    let (ci, _) = rings
        .iter()
        .enumerate()
        .max_by(|a, b| area(&a.1.uv).partial_cmp(&area(&b.1.uv)).unwrap())
        .unwrap();
    // winding of the candidate's polygon around a point; a point on the
    // boundary counts as inside (hole rims share the outer's corner vertices)
    let inside = |p: P2| -> bool {
        let uv = &rings[ci].uv;
        let n = uv.len();
        let mut sum = 0.0f64;
        for i in 0..n {
            let (a, b) = (uv[i], uv[(i + 1) % n]);
            let (ax, ay, bx, by) = (a[0] - p[0], a[1] - p[1], b[0] - p[0], b[1] - p[1]);
            let cross = ax * by - ay * bx;
            let dot = ax * bx + ay * by;
            if cross == 0.0 && dot <= 0.0 {
                return true;
            }
            sum += f64::atan2(cross, dot);
        }
        sum.abs() > std::f64::consts::FRAC_PI_2
    };
    let (t_u, t_v) = (surf.period_u(), surf.period_v());
    let cu = rings[ci]
        .uv
        .iter()
        .fold(f64::INFINITY, |acc, q| acc.min(q[0]));
    let cv = rings[ci]
        .uv
        .iter()
        .fold(f64::INFINITY, |acc, q| acc.min(q[1]));
    let ok = rings.iter().enumerate().all(|(i, r)| {
        i == ci
            || r.uv.iter().all(|&q| {
                let mut q = q;
                if t_u > 0.0 {
                    q[0] += t_u * ((cu - q[0]) / t_u).round();
                }
                if t_v > 0.0 {
                    q[1] += t_v * ((cv - q[1]) / t_v).round();
                }
                inside(q)
            })
    });
    if !ok {
        // two rims on a full-period surface (NIST CTC-01's lateral faces):
        // bounds are the two rim circles, no seam edge, neither marked outer
        // — the band they bound in (u,v) is the rectangle between them, walked
        // as one polyline (lower rim forwards, upper rim backwards, closed
        // implicitly at the seam), which is exactly one outer ring.
        if let Some(band) = band_ring(surf, rings, t_u, t_v) {
            rings.clear();
            rings.push(band);
            return Ok(());
        }
        return Err(format!(
            "face has {} bounds, none FACE_OUTER_BOUND, and no bound contains the others in (u,v)",
            rings.len()
        ));
    }
    rings[ci].is_outer = true;
    Ok(())
}

/// The two-rim band encoding: a lateral face of a full-period surface whose
/// bounds are the two rim circles (no seam edge, no `FACE_OUTER_BOUND` —
/// NIST CTC-01's cylinders). Neither rim contains the other in (u,v); in
/// (u,v) the face is the full-width strip between the two constants, so the
/// band polygon is built from the rims' own closed walks: each unwrapped
/// sweep (samples plus the implicit closure at ±one period) becomes one edge
/// of the strip, and the two seam edges are the strip's ends. Returns the
/// merged ring, window aligned, oriented CCW, or None when the bounds are
/// not a two-rim band (a rim must sit at a constant on one axis and sweep
/// exactly one period on the other, closing in 3D within about one sample
/// step — a rim that stops an arc short is real face boundary, not a full
/// circle, and the seam-less encoding would invent the missing wedge).
fn band_ring(surf: &Surf, rings: &[Ring], t_u: f64, t_v: f64) -> Option<Ring> {
    let axis_band = |surf: &Surf, period: f64, axis: usize, fixed: usize| -> Option<Ring> {
        if period <= 0.0 || rings.len() != 2 {
            return None;
        }
        // The resampled top edge and the seam corners come from evaluating the
        // surface directly; an exact analytic form makes (u, v_hi) land on the
        // rim circle. A clamped NURBS domain would not — refuse the shape.
        match surf.kind_name() {
            "cylinder" | "cone" | "sphere" | "torus" => {}
            _ => return None,
        }
        let wrap = |d: f64| -> f64 {
            let half = period * 0.5;
            let mut d = d;
            while d > half {
                d -= period;
            }
            while d < -half {
                d += period;
            }
            d
        };
        // per rim: unwrapped signed sweep over samples plus the implicit
        // closure step; stored u can be branch-lifted mid-array (a seam
        // point's far-branch start), so walk by wrapped deltas, not values
        let mut sweeps: Vec<(Vec<P3>, Vec<f64>, f64)> = Vec::new(); // (pts, unwrapped u, signed total)
        for r in rings {
            let fspan = r
                .uv
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| {
                    (lo.min(q[fixed]), hi.max(q[fixed]))
                });
            if fspan.1 - fspan.0 > period * 1e-3 {
                return None;
            }
            let n = r.uv.len();
            let us: Vec<f64> = (0..n).map(|i| r.uv[i][axis]).collect();
            let mut un = vec![us[0]];
            for i in 1..n {
                un.push(un[i - 1] + wrap(us[i] - us[i - 1]));
            }
            let total = un[n - 1] + wrap(un[0] - un[n - 1]) - un[0];
            if (total.abs() - period).abs() > period * 1e-3 {
                return None;
            }
            // the walk must be monotone on the sweep axis (either direction —
            // ascend() normalises below): a full-period rim circle is, so a
            // direction change means this is not a band boundary
            let mut dir = 0.0f64;
            for i in 1..n {
                let d = un[i] - un[i - 1];
                if dir == 0.0 {
                    dir = d;
                }
                if d * dir <= 0.0 {
                    return None;
                }
            }
            // closed in 3D within roughly one sample step: a rim that stops
            // an arc short is real face boundary, not a full circle
            let interior: f64 = r
                .pts
                .windows(2)
                .map(|w| dist3(w[0], w[1]))
                .sum::<f64>()
                .max(1e-12);
            let mean_step = interior / (n - 1).max(1) as f64;
            if dist3(*r.pts.first().unwrap(), *r.pts.last().unwrap()) > 1.5 * mean_step {
                return None;
            }
            sweeps.push((r.pts.clone(), un, total));
        }
        // order: rim at the lower constant first; both sweeps ascending
        // (bottom walk +u, top walk back −u)
        let fval = |r: &Ring| r.uv.iter().map(|q| q[fixed]).sum::<f64>() / r.uv.len() as f64;
        let (li, ti) = if fval(&rings[0]) <= fval(&rings[1]) {
            (0usize, 1usize)
        } else {
            (1, 0)
        };
        let ascend = |sw: &mut (Vec<P3>, Vec<f64>, f64)| {
            if sw.1.last().unwrap() < &sw.1[0] {
                sw.0.reverse();
                sw.1.reverse();
                sw.2 = -sw.2;
            }
        };
        ascend(&mut sweeps[li]);
        ascend(&mut sweeps[ti]);
        let bot = &sweeps[li];
        // the rims' own vertex phases differ in general (the reversed bottom
        // starts at its θ_n, the top at its θ_0), so whole-period window
        // alignment alone can misplace the top's edge by up to one sample
        // step — a real wedge out of the band at seam level. Only the
        // bottom's sampling survives: the top edge and both corner heads are
        // resampled at the bottom's own u values, surface-evaluated at the
        // top's constant.
        let _ = ti;
        let v_lo = fval(&rings[li]);
        let v_hi = fval(&rings[ti]);
        let band_pt = |u: f64| -> P3 {
            if axis == 0 {
                surf.eval(u, v_hi)
            } else {
                surf.eval(v_hi, u)
            }
        };
        // band polygon: the bottom rim's own samples (u sweeping one full
        // period at v_lo on the rim's own walk), its closure corner, the top
        // corner above it, then the top edge walked back
        let mut pts: Vec<P3> = Vec::with_capacity(2 * bot.0.len() + 2);
        let mut uv: Vec<P2> = Vec::with_capacity(2 * bot.0.len() + 2);
        for (q, u) in bot.0.iter().zip(bot.1.iter()) {
            pts.push(*q);
            uv.push(if axis == 0 { [*u, v_lo] } else { [v_lo, *u] });
        }
        // bottom's closure corner (same 3D as its first sample, one period over)
        let b_close = bot.1[0] + bot.2;
        pts.push(bot.0[0]);
        uv.push(if axis == 0 { [b_close, v_lo] } else { [v_lo, b_close] });
        // top's corner above it
        pts.push(band_pt(b_close));
        uv.push(if axis == 0 { [b_close, v_hi] } else { [v_hi, b_close] });
        for i in (0..bot.0.len()).rev() {
            let u = bot.1[i];
            pts.push(band_pt(u));
            uv.push(if axis == 0 { [u, v_hi] } else { [v_hi, u] });
        }
        // CCW in (u,v): the shoelace's sign decides; reverse if clockwise
        let mut sgn = 0.0f64;
        for i in 0..uv.len() {
            let (a, b) = (uv[i], uv[(i + 1) % uv.len()]);
            sgn += a[0] * b[1] - b[0] * a[1];
        }
        if sgn < 0.0 {
            uv.reverse();
            pts.reverse();
        }
        Some(Ring {
            pts,
            uv,
            is_outer: true,
        })
    };
    axis_band(surf, t_u, 0, 1).or_else(|| axis_band(surf, t_v, 1, 0))
}

/// The reader's name for a curve kind, for the named-refusal report.
fn edge_label<'m>(k: CurveKind<'m>) -> String {
    match k {
        CurveKind::Line(_) => "LINE".into(),
        CurveKind::Circle(_) => "CIRCLE".into(),
        CurveKind::Ellipse(_) => "ELLIPSE".into(),
        CurveKind::BSpline(_) => "B_SPLINE_CURVE (without knot data)".into(),
        CurveKind::BSplineWithKnots(_) => "B_SPLINE_CURVE_WITH_KNOTS".into(),
        CurveKind::Trimmed(_) => "TRIMMED_CURVE".into(),
        CurveKind::Polyline(_) => "POLYLINE".into(),
        CurveKind::Composite(_) => "COMPOSITE_CURVE".into(),
        CurveKind::QuasiUniform(_) => "QUASI_UNIFORM_CURVE".into(),
        CurveKind::Uniform(_) => "UNIFORM_CURVE".into(),
        CurveKind::Bezier(_) => "BEZIER_CURVE".into(),
        CurveKind::SurfaceCurve(_) => "SURFACE_CURVE".into(),
        CurveKind::SeamCurve(_) => "SEAM_CURVE".into(),
        CurveKind::BoundedSurfaceCurve(_) => "BOUNDED_SURFACE_CURVE".into(),
        CurveKind::IntersectionCurve(_) => "INTERSECTION_CURVE".into(),
        CurveKind::Other(kw) => kw.to_string(),
    }
}
// ---------------------------------------------------------------------------
// fixtures: the tessellator verified against analytically-authored STEP files
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use step_io::build::{CurveInput, FaceBoundInput, Frame as BFrame, SurfaceInput, UnitsInput};
    use step_io::generated::model as m;

    /// The volume of a closed triangle mesh by divergence (Σ a·(b×c)/6),
    /// sign positive for outward windings.
    pub fn mesh_volume(m: &TriMesh) -> f64 {
        let mut v = 0.0;
        for t in &m.tris {
            let a = m.verts[t[0] as usize];
            let b = m.verts[t[1] as usize];
            let c = m.verts[t[2] as usize];
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let x = [ab[1] * ac[2] - ab[2] * ac[1], ab[2] * ac[0] - ab[0] * ac[2], ab[0] * ac[1] - ab[1] * ac[0]];
            v += (a[0] * x[0] + a[1] * x[1] + a[2] * x[2]) / 6.0;
        }
        v
    }

    /// Author a STEP file text with the step-io builder and return its bytes.
    fn build(f: impl FnOnce(&mut step_io::build::StepBuilder)) -> Vec<u8> {
        let mut b = step_io::build::StepBuilder::new().expect("builder");
        f(&mut b);
        b.finish().expect("finish").into_bytes()
    }

    /// An axis-aligned box [0,0,0]..[x,y,z] with outward face normals.
    fn box_faces(b: &mut step_io::build::StepBuilder, x: f64, y: f64, z: f64) {
        let vs: Vec<_> = [
            [0.0, 0.0, 0.0],
            [x, 0.0, 0.0],
            [x, y, 0.0],
            [0.0, y, 0.0],
            [0.0, 0.0, z],
            [x, 0.0, z],
            [x, y, z],
            [0.0, y, z],
        ]
        .iter()
        .map(|p| b.vertex(*p).unwrap())
        .collect();
        let mut e = |from: usize, to: usize| b.edge(vs[from], vs[to], CurveInput::Line).unwrap();
        // 12 canonical edge entities — a manifold shell uses each exactly twice,
        // so the face bounds below reference them with per-occurrence direction
        // flags rather than authoring one entity per occurrence.
        let e01 = e(0, 1);
        let e12 = e(1, 2);
        let e23 = e(2, 3);
        let e03 = e(0, 3);
        let e45 = e(4, 5);
        let e56 = e(5, 6);
        let e67 = e(6, 7);
        let e74 = e(7, 4);
        let e15 = e(1, 5);
        let e26 = e(2, 6);
        let e37 = e(3, 7);
        let e04 = e(0, 4);
        let bottom = [(e01, true), (e12, true), (e23, true), (e03, false)];
        let top = [(e45, true), (e56, true), (e67, true), (e74, true)];
        let side_x0 = [(e03, true), (e37, true), (e74, true), (e04, false)];
        let side_x1 = [(e12, true), (e26, true), (e56, false), (e15, false)];
        let side_y0 = [(e01, true), (e15, true), (e45, false), (e04, false)];
        let side_y1 = [(e23, false), (e26, true), (e67, true), (e37, false)];
        let fr = |origin: [f64; 3], axis: [f64; 3], rf: [f64; 3]| BFrame {
            origin,
            axis,
            ref_dir: rf,
        };
        let mut face = |surface: SurfaceInput, same_sense: bool, edges: &[(m::EdgeCurveId, bool)]| {
            b.face(surface, same_sense, vec![FaceBoundInput::outer(edges.to_vec())]).unwrap()
        };
        let f = face(SurfaceInput::Plane(fr([0.0, 0.0, z], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0])), true, &top);
        let g = face(SurfaceInput::Plane(fr([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0])), false, &bottom);
        let h = face(SurfaceInput::Plane(fr([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])), false, &side_x0);
        let i = face(SurfaceInput::Plane(fr([x, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])), true, &side_x1);
        let j = face(SurfaceInput::Plane(fr([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0])), false, &side_y0);
        let k = face(SurfaceInput::Plane(fr([0.0, y, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0])), true, &side_y1);
        let part = b.part("box").unwrap();
        b.solid(part, "box", vec![f, g, h, i, j, k]).unwrap();
    }

    #[test]
    fn box_tessellates_watertight_with_unit_volume() {
        let bytes = build(|b| box_faces(b, 2.0, 3.0, 4.0));
        let doc = tessellate(&bytes, None).expect("box tessellates");
        assert_eq!(doc.parts.len(), 1);
        assert_eq!(doc.length_unit, "MILLIMETRE");
        let p = &doc.parts[0];
        assert_eq!(p.faces, 6);
        assert_eq!(p.name, "box");
        let v = p.volume.expect("all-planar faces have flux");
        assert!((v - 24.0).abs() < 1e-6, "flux volume {v} vs 24");
        let mv = mesh_volume(&p.mesh);
        assert!((mv - 24.0).abs() < 1e-6, "mesh divergence volume {mv} vs 24");
        assert_eq!(p.tris, 12);
    }

    #[test]
    fn metre_box_scales_to_mm() {
        let mut b = step_io::build::StepBuilder::new_with(&UnitsInput {
            length: step_io::build::LengthUnit::Metre,
            uncertainty: 1.0e-7,
        })
        .unwrap();
        box_faces(&mut b, 0.1, 0.2, 0.3);
        let doc = tessellate(&b.finish().unwrap().into_bytes(), None).unwrap();
        assert_eq!(doc.length_unit, "METRE");
        assert_eq!(doc.parts.len(), 1);
        let v = doc.parts[0].volume.expect("metre box has flux");
        assert!((v - 6.0e6).abs() < 1.0, "0.1×0.2×0.3 m is 6e6 mm³, got {v}");
        let expect_chord = (140_000.0f64.sqrt()) * 3e-4;
        assert!(
            (doc.chord - expect_chord).abs() < 0.01,
            "chord {} vs diagonal·3e-4 = {expect_chord}", doc.chord,
        );
    }

    #[test]
    fn full_cylinder_with_seam_tessellates_whole() {
        // the classic encoding: lateral face bound = bottom seam circle (occ),
        // seam line fwd, top circle, seam line back — the two seam-line
        // occurrences are one entity twice; circles are shared with the caps.
        let bytes = build(|b| {
            let (r, h) = (5.0_f64, 10.0);
            let v_bot = b.vertex([r, 0.0, 0.0]).unwrap();
            let v_top = b.vertex([r, 0.0, h]).unwrap();
            let f_circ = |origin: [f64; 3]| BFrame { origin, axis: [0.0, 0.0, 1.0], ref_dir: [1.0, 0.0, 0.0] };
            let c_bot = b.edge(v_bot, v_bot, CurveInput::Circle(f_circ([0.0, 0.0, 0.0]), r)).unwrap();
            let c_top = b.edge(v_top, v_top, CurveInput::Circle(f_circ([0.0, 0.0, h]), r)).unwrap();
            let seam = b.edge(v_bot, v_top, CurveInput::Line).unwrap();
            let wall = b
                .face(
                    SurfaceInput::Cylinder(f_circ([0.0, 0.0, 0.0]), r),
                    true,
                    vec![FaceBoundInput::outer(vec![(c_bot, true), (seam, true), (c_top, false), (seam, false)])],
                )
                .unwrap();
            let cap_bot = b
                .face(
                    SurfaceInput::Plane(f_circ([0.0, 0.0, 0.0])), // axis +z: natural normal +z = inward for the bottom cap
                    false,
                    vec![FaceBoundInput::outer(vec![(c_bot, false)])],
                )
                .unwrap();
            let cap_top = b
                .face(
                    SurfaceInput::Plane(f_circ([0.0, 0.0, h])),
                    true,
                    vec![FaceBoundInput::outer(vec![(c_top, true)])],
                )
                .unwrap();
            let part = b.part("cyl").unwrap();
            b.solid(part, "cyl", vec![wall, cap_bot, cap_top]).unwrap();
        });
        let doc = tessellate(&bytes, Some(0.01)).unwrap();
        assert_eq!(doc.parts.len(), 1);
        let p = &doc.parts[0];
        let v = p.volume.expect("analytic faces have flux");
        let expect = std::f64::consts::PI * 25.0 * 10.0;
        // the flux quadrature is exact over the sampled boundary (the wall's
        // constant integrand reproduces 2πr²h/3 to ~1e-15); what remains is the
        // inscribed-polyline shortfall of the cap's ring, linear in the chord
        assert!(
            (v - expect).abs() / expect < 2.5e-3,
            "volume {v} vs {expect} at chord 0.01"
        );
        let mv = mesh_volume(&p.mesh);
        assert!((mv - expect).abs() / expect < 2e-3, "mesh divergence {mv} vs {expect}");
        assert!(p.tris >= 3, "got {} tris", p.tris);
        assert!(
            p.mesh_area / p.quad_area > 0.999,
            "coverage {} (mesh {} quad {})",
            p.mesh_area / p.quad_area,
            p.mesh_area,
            p.quad_area
        );
        // halving the chord quarters the shortfall (0.1616% → ~0.040%)
        let doc_fine = tessellate(&bytes, Some(0.0025)).unwrap();
        let v_fine = doc_fine.parts[0].volume.expect("fine chord has flux");
        assert!(
            (v_fine - expect).abs() / expect < 8e-4,
            "volume {v_fine} vs {expect} at chord 0.0025"
        );
    }

    #[test]
    fn nurbs_bilinear_box_is_watertight_without_flux() {
        // six 2×2-weight-1 bilinear patches, each spanning one box face
        let bytes = build(|b| {
            let k = [0.0, 0.0, 1.0, 1.0];
            let (x, y, z) = (2.0_f64, 3.0, 4.0);
            let p = |i: usize, j: usize, k2: usize| [x * i as f64, y * j as f64, z * k2 as f64];
            // edges: 12 straight edges between the 8 corners
            let vs: Vec<_> = [
                [0.0, 0.0, 0.0],
                [x, 0.0, 0.0],
                [x, y, 0.0],
                [0.0, y, 0.0],
                [0.0, 0.0, z],
                [x, 0.0, z],
                [x, y, z],
                [0.0, y, z],
            ]
            .iter()
            .map(|p| b.vertex(*p).unwrap())
            .collect();
            let mut e = |from: usize, to: usize| b.edge(vs[from], vs[to], CurveInput::Line).unwrap();
            let e01 = e(0, 1);
            let e12 = e(1, 2);
            let e23 = e(2, 3);
            let e03 = e(0, 3);
            let e45 = e(4, 5);
            let e56 = e(5, 6);
            let e67 = e(6, 7);
            let e74 = e(7, 4);
            let e15 = e(1, 5);
            let e26 = e(2, 6);
            let e37 = e(3, 7);
            let e04 = e(0, 4);
            let bottom = [(e01, true), (e12, true), (e23, true), (e03, false)];
            let top = [(e45, true), (e56, true), (e67, true), (e74, true)];
            let side_x0 = [(e03, true), (e37, true), (e74, true), (e04, false)];
            let side_x1 = [(e12, true), (e26, true), (e56, false), (e15, false)];
            let side_y0 = [(e01, true), (e15, true), (e45, false), (e04, false)];
            let side_y1 = [(e23, false), (e26, true), (e67, true), (e37, false)];
            let q = |cps: [[f64; 3]; 4]| step_io::scene::NurbsSurface {
                degree_u: 1,
                degree_v: 1,
                control_points: vec![vec![cps[0], cps[1]], vec![cps[2], cps[3]]],
                weights: vec![vec![1.0, 1.0], vec![1.0, 1.0]],
                knots_u: k.to_vec(),
                knots_v: k.to_vec(),
            };
            // each patch maps [0,1]² affinely onto one box face: cps rows are
            // [P0, P1; P2, P3] with a = P1−P0, b = P2−P0 the two edge vectors and
            // P0+P3 = P1+P2; the patch's natural normal is b×a, so each face's
            // cps are ordered to point the encoded side (n_nat outward where
            // same_sense = true, inward where false)
            let f = b.face(SurfaceInput::Nurbs(q([p(0, 0, 1), p(0, 1, 1), p(1, 0, 1), p(1, 1, 1)])), true, vec![FaceBoundInput::outer(top.to_vec())]).unwrap();
            let g = b.face(SurfaceInput::Nurbs(q([p(0, 0, 0), p(0, 1, 0), p(1, 0, 0), p(1, 1, 0)])), false, vec![FaceBoundInput::outer(bottom.to_vec())]).unwrap();
            let h = b.face(SurfaceInput::Nurbs(q([p(0, 0, 0), p(0, 0, 1), p(0, 1, 0), p(0, 1, 1)])), false, vec![FaceBoundInput::outer(side_x0.to_vec())]).unwrap();
            let i = b.face(SurfaceInput::Nurbs(q([p(1, 0, 0), p(1, 0, 1), p(1, 1, 0), p(1, 1, 1)])), true, vec![FaceBoundInput::outer(side_x1.to_vec())]).unwrap();
            let j = b.face(SurfaceInput::Nurbs(q([p(0, 0, 0), p(1, 0, 0), p(0, 0, 1), p(1, 0, 1)])), false, vec![FaceBoundInput::outer(side_y0.to_vec())]).unwrap();
            let k3 = b.face(SurfaceInput::Nurbs(q([p(0, 1, 0), p(1, 1, 0), p(0, 1, 1), p(1, 1, 1)])), true, vec![FaceBoundInput::outer(side_y1.to_vec())]).unwrap();
            let part = b.part("nbox").unwrap();
            b.solid(part, "nbox", vec![f, g, h, i, j, k3]).unwrap();
        });
        let doc = tessellate(&bytes, Some(0.01)).unwrap();
        assert_eq!(doc.parts.len(), 1);
        let p = &doc.parts[0];
        assert!(p.volume.is_none(), "NURBS faces have no flux potential");
        assert!((p.quad_area - 52.0).abs() < 1e-6, "2(2·3+2·4+3·4) = 52, got {}", p.quad_area);
        let mv = mesh_volume(&p.mesh);
        assert!((mv - 24.0).abs() < 1e-6, "mesh divergence {mv} vs 24");
        assert_eq!(p.tris, 12);
    }

    #[test]
    fn bad_bytes_and_bad_chord_refuse() {
        assert!(tessellate(b"garbage not step", None).is_err());
        let bytes = build(|b| box_faces(b, 2.0, 3.0, 4.0));
        assert!(tessellate(&bytes, Some(0.0)).is_err());
        assert!(tessellate(&bytes, Some(-1.0)).is_err());
        // 0.0 chord is refused by name, a tiny positive one is honoured
        let doc = tessellate(&bytes, Some(1e-4)).unwrap();
        assert_eq!(doc.parts.len(), 1);
    }
}
