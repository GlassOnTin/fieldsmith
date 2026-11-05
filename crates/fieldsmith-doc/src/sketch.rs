//! Parametric 2D Sketch & Constraint Solver + Surface-Sketch 3D Feature Engine.
//!
//! Designed for both interactive CAD users (Shapr3D / Onshape / SolveSpace / FreeCAD
//! workflows) and declarative MCP / AI agents:
//! - Damped Levenberg–Marquardt (LM) nonlinear least-squares solver with homotopy
//!   continuation sub-stepping so large dimensional jumps never flip sketch branches.
//! - Column-pivoted Householder QR rank decomposition on the analytic/central-difference
//!   Jacobian `J` (`m × n`), yielding exact degrees-of-freedom (`DOF = n_free - rank(J)`),
//!   per-point freedom status (`Fixed`, `Constrained`, `UnderConstrained`, `Conflicting`),
//!   redundant-constraint identification (zero pivot rows with `|r_i| ≤ tol`), and
//!   conflicting-constraint identification (inconsistent rows with `|r_i| > tol`).
//! - 12 geometric constraints (`Coincident`, `Fixed`, `Horizontal`, `Vertical`, `Parallel`,
//!   `Perpendicular`, `EqualLength`, `Tangent`, `Concentric`, `Midpoint`, `Symmetric`,
//!   `PointOnLine`) and 6 dimensional constraints (`Distance`, `HorizontalDistance`,
//!   `VerticalDistance`, `Angle`, `Radius`, `Diameter`) supporting `.fsm` `let` expressions.
//! - High-level shape macros (`rect`, `center_rect`, `circle`, `slot`, `polygon`,
//!   `fillet_corner`, `bolt_circle`) and engineering preset templates.
//! - Surface-attached 3D feature compiler (`BossExtrude`, `PocketCut`, `RevolveBoss`,
//!   `RevolveCut`, `HolePattern`) resolving `on = "<shape>.<face>"` or world datum planes.

use crate::fsm::eval_expr;
use crate::model::{Document, Face, Let, Node, NodeKind, Op, ProfileVertex, Transform};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

// ---------------------------------------------------------------------------------------------
// Data Model: Sketch Plane, Entities, Constraints, Macros
// ---------------------------------------------------------------------------------------------

/// Coordinate frame where a 2D sketch `(u, v)` lives in 3D space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchPlane {
    /// World XY plane (`z = offset`, `u = +X`, `v = +Y`, normal `+Z`).
    Xy {
        #[serde(default)]
        offset: f64,
    },
    /// World XZ plane (`y = offset`, `u = +X`, `v = +Z`, normal `-Y` / `+Y` extrude).
    Xz {
        #[serde(default)]
        offset: f64,
    },
    /// World YZ plane (`x = offset`, `u = +Y`, `v = +Z`, normal `+X`).
    Yz {
        #[serde(default)]
        offset: f64,
    },
    /// Attached directly to a named 3D solid's face (`e.g. "base.top"`, `"upright.px"`).
    OnFace {
        host_id: String,
        face: Face,
        #[serde(default)]
        offset: f64,
    },
}

impl Default for SketchPlane {
    fn default() -> Self {
        SketchPlane::Xy { offset: 0.0 }
    }
}

impl SketchPlane {
    /// Parse shorthand `"xy"`, `"xz"`, `"yz"`, or `"<host_id>.<face>"`.
    pub fn parse_spec(spec: &str, offset: f64) -> Result<Self, String> {
        let s = spec.trim();
        match s.to_ascii_lowercase().as_str() {
            "" | "xy" | "top" => return Ok(SketchPlane::Xy { offset }),
            "xz" | "front" => return Ok(SketchPlane::Xz { offset }),
            "yz" | "right" => return Ok(SketchPlane::Yz { offset }),
            _ => {}
        }
        if let Some((host, face_str)) = s.split_once('.') {
            let face = match face_str.to_ascii_lowercase().as_str() {
                "+x" | "px" => Face::Px,
                "-x" | "nx" => Face::Nx,
                "+y" | "py" => Face::Py,
                "-y" | "ny" => Face::Ny,
                "+z" | "pz" => Face::Pz,
                "-z" | "nz" => Face::Nz,
                "top" => Face::Top,
                "bottom" => Face::Bottom,
                "side" => Face::Side,
                other => {
                    return Err(format!(
                        "unknown sketch attachment face '{other}' in '{spec}' (expected top, bottom, +x/-x/+y/-y/+z/-z)"
                    ))
                }
            };
            return Ok(SketchPlane::OnFace {
                host_id: host.trim().to_string(),
                face,
                offset,
            });
        }
        Err(format!(
            "invalid sketch plane spec '{spec}': expected 'xy', 'xz', 'yz', or '<shape_id>.<face>'"
        ))
    }
}

/// 2D point in sketch `(u, v)` coordinates (mm).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchPoint {
    pub id: String,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub fixed: bool,
    #[serde(default)]
    pub construction: bool,
}

/// Line or circular arc segment between two `SketchPoint`s (`bulge = tan(θ / 4)`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchSegment {
    pub id: String,
    pub p1: String,
    pub p2: String,
    /// `0.0` for a straight line; `tan(sweep_angle / 4)` for a circular arc (`1.0` = CCW semicircle).
    #[serde(default)]
    pub bulge: f64,
    #[serde(default)]
    pub construction: bool,
}

/// Standalone full circle centered at a `SketchPoint` (compiled into a 2-semicircle closed loop
/// or used as construction geometry such as a bolt circle).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchCircle {
    pub id: String,
    pub center: String,
    pub radius: f64,
    #[serde(default)]
    pub construction: bool,
}

/// 12 geometric + 6 dimensional constraint kinds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchConstraintKind {
    /// Two points occupy the same `(x, y)` coordinates (2 scalar equations).
    Coincident { p1: String, p2: String },
    /// Lock a point at `(x, y)` (2 scalar equations).
    Fixed { point: String, x: f64, y: f64 },
    /// Force a segment or pair of points to have `Δy = 0` (1 scalar equation).
    Horizontal {
        #[serde(default)]
        segment: Option<String>,
        #[serde(default)]
        p1: Option<String>,
        #[serde(default)]
        p2: Option<String>,
    },
    /// Force a segment or pair of points to have `Δx = 0` (1 scalar equation).
    Vertical {
        #[serde(default)]
        segment: Option<String>,
        #[serde(default)]
        p1: Option<String>,
        #[serde(default)]
        p2: Option<String>,
    },
    /// Two segments have zero cross-product direction `u1 × u2 = 0` (1 scalar equation).
    Parallel { seg1: String, seg2: String },
    /// Two segments have zero dot-product direction `u1 · u2 = 0` (1 scalar equation).
    Perpendicular { seg1: String, seg2: String },
    /// Two segments have equal chord/arc length or two circles have equal radius (1 scalar equation).
    EqualLength { seg1: String, seg2: String },
    /// Segment is tangent to a circle or arc segment (1 scalar equation).
    Tangent { seg: String, target: String },
    /// Two circles or arc segments share the same center (2 scalar equations).
    Concentric { c1: String, c2: String },
    /// Point `mid` lies at the midpoint `(p1 + p2) / 2` of a segment or point pair (2 scalar equations).
    Midpoint {
        point: String,
        #[serde(default)]
        segment: Option<String>,
        #[serde(default)]
        p1: Option<String>,
        #[serde(default)]
        p2: Option<String>,
    },
    /// Points `p1` and `p2` are symmetric across a symmetry axis segment `axis_seg` or `X`/`Y` axis (2 scalar equations).
    Symmetric {
        p1: String,
        p2: String,
        /// Either a segment id, `"x_axis"` (y=0), or `"y_axis"` (x=0).
        axis: String,
    },
    /// Point `point` lies on the infinite line through `segment` (1 scalar equation).
    PointOnLine { point: String, segment: String },
    /// Euclidean distance between two points or length of a segment (1 scalar equation).
    Distance {
        #[serde(default)]
        segment: Option<String>,
        #[serde(default)]
        p1: Option<String>,
        #[serde(default)]
        p2: Option<String>,
        value: f64,
        #[serde(default)]
        expr: Option<String>,
    },
    /// Signed or unsigned horizontal distance `|x2 - x1| = value` (1 scalar equation).
    HorizontalDistance {
        #[serde(default)]
        segment: Option<String>,
        #[serde(default)]
        p1: Option<String>,
        #[serde(default)]
        p2: Option<String>,
        value: f64,
        #[serde(default)]
        expr: Option<String>,
    },
    /// Signed or unsigned vertical distance `|y2 - y1| = value` (1 scalar equation).
    VerticalDistance {
        #[serde(default)]
        segment: Option<String>,
        #[serde(default)]
        p1: Option<String>,
        #[serde(default)]
        p2: Option<String>,
        value: f64,
        #[serde(default)]
        expr: Option<String>,
    },
    /// Angle in degrees between two segments or between a segment and the `+X` axis (1 scalar equation).
    Angle {
        seg1: String,
        #[serde(default)]
        seg2: Option<String>,
        degrees: f64,
        #[serde(default)]
        expr: Option<String>,
    },
    /// Radius of a circle or circular arc segment (1 scalar equation).
    Radius {
        target: String,
        value: f64,
        #[serde(default)]
        expr: Option<String>,
    },
    /// Diameter (`2 * R`) of a circle or circular arc segment (1 scalar equation).
    Diameter {
        target: String,
        value: f64,
        #[serde(default)]
        expr: Option<String>,
    },
}

/// Named constraint in a `ParametricSketch`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchConstraint {
    pub id: String,
    #[serde(flatten)]
    pub kind: SketchConstraintKind,
}

/// High-level declarative macro that expands into points, segments, circles, and constraints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "macro", rename_all = "snake_case")]
pub enum SketchMacro {
    /// Axis-aligned rectangle from corner `(x, y)` with `width × height` and optional corner fillet.
    Rect {
        id: String,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        #[serde(default)]
        fillet: f64,
        #[serde(default)]
        construction: bool,
    },
    /// Center-symmetric rectangle centered at `(cx, cy)` with `width × height` and optional corner fillet.
    CenterRect {
        id: String,
        cx: f64,
        cy: f64,
        width: f64,
        height: f64,
        #[serde(default)]
        fillet: f64,
        #[serde(default)]
        construction: bool,
    },
    /// Full circle centered at `(cx, cy)` with `radius`.
    Circle {
        id: String,
        cx: f64,
        cy: f64,
        radius: f64,
        #[serde(default)]
        construction: bool,
    },
    /// Stadium / obround slot between centers `(x1, y1)` and `(x2, y2)` with cap `radius`.
    Slot {
        id: String,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        radius: f64,
    },
    /// Regular `sides`-gon inscribed in or circumscribed around `radius` at `(cx, cy)`.
    Polygon {
        id: String,
        cx: f64,
        cy: f64,
        radius: f64,
        sides: usize,
        #[serde(default)]
        angle_deg: f64,
        #[serde(default)]
        construction: bool,
    },
    /// Bolt-hole circle: a construction pitch circle of `pcd` diameter plus `count` hole circles of `hole_diameter`.
    BoltCircle {
        id: String,
        cx: f64,
        cy: f64,
        pcd: f64,
        count: usize,
        hole_diameter: f64,
        #[serde(default)]
        start_angle_deg: f64,
    },
    /// Replace a sharp corner point shared by two straight segments with a tangent arc of `radius`.
    FilletCorner {
        point_id: String,
        radius: f64,
    },
}

/// Complete parametric 2D sketch definition.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParametricSketch {
    #[serde(default = "default_sketch_id")]
    pub id: String,
    #[serde(default)]
    pub plane: SketchPlane,
    #[serde(default)]
    pub points: Vec<SketchPoint>,
    #[serde(default)]
    pub segments: Vec<SketchSegment>,
    #[serde(default)]
    pub circles: Vec<SketchCircle>,
    #[serde(default)]
    pub constraints: Vec<SketchConstraint>,
    #[serde(default)]
    pub macros: Vec<SketchMacro>,
}

fn default_sketch_id() -> String {
    "sketch1".to_string()
}

// ---------------------------------------------------------------------------------------------
// Solver Diagnostics & Report
// ---------------------------------------------------------------------------------------------

/// Freedom classification for a sketch entity or the overall sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SketchStatus {
    /// `DOF == 0` and all constraint residuals are within tolerance (`≤ 1e-6`).
    FullyConstrained,
    /// `DOF > 0` and all constraint residuals are within tolerance (`≤ 1e-6`).
    UnderConstrained,
    /// `DOF == 0` (or `> 0`) with redundant constraints that are consistent (`|r_i| ≤ 1e-6`).
    RedundantConstraints,
    /// One or more constraints conflict and cannot be simultaneously satisfied (`|r_i| > 1e-4`).
    OverConstrainedConflict,
}

/// Per-point freedom state for color-coding in the UI and MCP agent feedback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointFreedom {
    pub id: String,
    pub x: f64,
    pub y: f64,
    /// `0` = fully locked, `1` = slides along a 1D curve/line, `2` = free in 2D plane.
    pub dof: usize,
    pub status: SketchStatus,
}

/// Complete diagnostic report returned by [`solve_sketch`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchSolveReport {
    pub status: SketchStatus,
    pub converged: bool,
    pub iterations: usize,
    pub max_residual: f64,
    pub l2_residual: f64,
    /// Total free scalar coordinates (`2 * N_points + N_arc_bulges + N_circle_radii`).
    pub total_variables: usize,
    /// Total scalar constraint equations (`m`).
    pub total_equations: usize,
    /// Numerical rank of the constraint Jacobian `J` via column-pivoted Householder QR.
    pub jacobian_rank: usize,
    /// Remaining degrees of freedom (`total_variables - jacobian_rank`).
    pub dof: usize,
    /// IDs of constraints that are mathematically redundant (dependent rows of `J`, consistent residual).
    pub redundant_constraints: Vec<String>,
    /// IDs of constraints with non-zero residual after LM convergence (conflicting constraints).
    pub conflicting_constraints: Vec<String>,
    /// Actionable human- and LLM-readable diagnosis message.
    pub diagnosis: String,
    /// Per-point freedom breakdown.
    pub point_freedom: Vec<PointFreedom>,
    /// Solved outer closed profile `(x, y, bulge)` ready for `NodeKind::Extrude` or `NodeKind::Revolve`.
    pub outer_profile: Vec<ProfileVertex>,
    /// Solved interior hole profiles `(x, y, bulge)` (e.g., bolt circle holes or inner cutouts).
    pub hole_profiles: Vec<Vec<ProfileVertex>>,
    /// Solved sketch after macro expansion and LM optimization.
    pub solved_sketch: ParametricSketch,
}

// ---------------------------------------------------------------------------------------------
// Macro Expansion & Preset Templates
// ---------------------------------------------------------------------------------------------

impl ParametricSketch {
    /// Expand any declarative `macros` in `self.macros` into concrete points, segments,
    /// circles, and constraints, clearing `self.macros` once expanded.
    pub fn expand_macros(&mut self) -> Result<(), String> {
        let macros = std::mem::take(&mut self.macros);
        for m in macros {
            match m {
                SketchMacro::Rect {
                    id,
                    x,
                    y,
                    width,
                    height,
                    fillet,
                    construction,
                } => {
                    self.add_rect_macro(&id, x, y, width, height, fillet, construction)?;
                }
                SketchMacro::CenterRect {
                    id,
                    cx,
                    cy,
                    width,
                    height,
                    fillet,
                    construction,
                } => {
                    self.add_center_rect_macro(&id, cx, cy, width, height, fillet, construction)?;
                }
                SketchMacro::Circle {
                    id,
                    cx,
                    cy,
                    radius,
                    construction,
                } => {
                    self.add_circle_macro(&id, cx, cy, radius, construction)?;
                }
                SketchMacro::Slot {
                    id,
                    x1,
                    y1,
                    x2,
                    y2,
                    radius,
                } => {
                    self.add_slot_macro(&id, x1, y1, x2, y2, radius)?;
                }
                SketchMacro::Polygon {
                    id,
                    cx,
                    cy,
                    radius,
                    sides,
                    angle_deg,
                    construction,
                } => {
                    self.add_polygon_macro(&id, cx, cy, radius, sides, angle_deg, construction)?;
                }
                SketchMacro::BoltCircle {
                    id,
                    cx,
                    cy,
                    pcd,
                    count,
                    hole_diameter,
                    start_angle_deg,
                } => {
                    self.add_bolt_circle_macro(
                        &id,
                        cx,
                        cy,
                        pcd,
                        count,
                        hole_diameter,
                        start_angle_deg,
                    )?;
                }
                SketchMacro::FilletCorner { point_id, radius } => {
                    self.fillet_sketch_corner(&point_id, radius)?;
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_rect_macro(
        &mut self,
        id: &str,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        fillet: f64,
        construction: bool,
    ) -> Result<(), String> {
        if width <= 0.0 || height <= 0.0 {
            return Err(format!("rect '{id}' width and height must be positive"));
        }
        let p0 = format!("{id}_p0");
        let p1 = format!("{id}_p1");
        let p2 = format!("{id}_p2");
        let p3 = format!("{id}_p3");
        self.points.push(SketchPoint {
            id: p0.clone(),
            x,
            y,
            fixed: false,
            construction,
        });
        self.points.push(SketchPoint {
            id: p1.clone(),
            x: x + width,
            y,
            fixed: false,
            construction,
        });
        self.points.push(SketchPoint {
            id: p2.clone(),
            x: x + width,
            y: y + height,
            fixed: false,
            construction,
        });
        self.points.push(SketchPoint {
            id: p3.clone(),
            x,
            y: y + height,
            fixed: false,
            construction,
        });

        let s_bot = format!("{id}_bot");
        let s_right = format!("{id}_right");
        let s_top = format!("{id}_top");
        let s_left = format!("{id}_left");
        self.segments.push(SketchSegment {
            id: s_bot.clone(),
            p1: p0.clone(),
            p2: p1.clone(),
            bulge: 0.0,
            construction,
        });
        self.segments.push(SketchSegment {
            id: s_right.clone(),
            p1: p1.clone(),
            p2: p2.clone(),
            bulge: 0.0,
            construction,
        });
        self.segments.push(SketchSegment {
            id: s_top.clone(),
            p1: p2.clone(),
            p2: p3.clone(),
            bulge: 0.0,
            construction,
        });
        self.segments.push(SketchSegment {
            id: s_left.clone(),
            p1: p3.clone(),
            p2: p0.clone(),
            bulge: 0.0,
            construction,
        });

        // Fully constrain the rectangle (8 variables for 4 points):
        // 1. Anchor p0 (2 eqs)
        // 2. Horizontal bottom & top (2 eqs)
        // 3. Vertical right & left (2 eqs)
        // 4. Width & Height distances (2 eqs) -> Total 8 independent equations!
        self.constraints.push(SketchConstraint {
            id: format!("{id}_fix_origin"),
            kind: SketchConstraintKind::Fixed {
                point: p0,
                x,
                y,
            },
        });
        self.constraints.push(SketchConstraint {
            id: format!("{id}_horiz_bot"),
            kind: SketchConstraintKind::Horizontal {
                segment: Some(s_bot.clone()),
                p1: None,
                p2: None,
            },
        });
        self.constraints.push(SketchConstraint {
            id: format!("{id}_horiz_top"),
            kind: SketchConstraintKind::Horizontal {
                segment: Some(s_top),
                p1: None,
                p2: None,
            },
        });
        self.constraints.push(SketchConstraint {
            id: format!("{id}_vert_right"),
            kind: SketchConstraintKind::Vertical {
                segment: Some(s_right.clone()),
                p1: None,
                p2: None,
            },
        });
        self.constraints.push(SketchConstraint {
            id: format!("{id}_vert_left"),
            kind: SketchConstraintKind::Vertical {
                segment: Some(s_left),
                p1: None,
                p2: None,
            },
        });
        self.constraints.push(SketchConstraint {
            id: format!("{id}_width"),
            kind: SketchConstraintKind::Distance {
                segment: Some(s_bot),
                p1: None,
                p2: None,
                value: width,
                expr: None,
            },
        });
        self.constraints.push(SketchConstraint {
            id: format!("{id}_height"),
            kind: SketchConstraintKind::Distance {
                segment: Some(s_right),
                p1: None,
                p2: None,
                value: height,
                expr: None,
            },
        });

        if fillet > 1e-6 {
            for corner in [
                format!("{id}_p0"),
                format!("{id}_p1"),
                format!("{id}_p2"),
                format!("{id}_p3"),
            ] {
                self.fillet_sketch_corner(&corner, fillet)?;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_center_rect_macro(
        &mut self,
        id: &str,
        cx: f64,
        cy: f64,
        width: f64,
        height: f64,
        fillet: f64,
        construction: bool,
    ) -> Result<(), String> {
        self.add_rect_macro(
            id,
            cx - 0.5 * width,
            cy - 0.5 * height,
            width,
            height,
            fillet,
            construction,
        )
    }

    pub fn add_circle_macro(
        &mut self,
        id: &str,
        cx: f64,
        cy: f64,
        radius: f64,
        construction: bool,
    ) -> Result<(), String> {
        if radius <= 0.0 {
            return Err(format!("circle '{id}' radius must be positive"));
        }
        let pc = format!("{id}_center");
        self.points.push(SketchPoint {
            id: pc.clone(),
            x: cx,
            y: cy,
            fixed: false,
            construction: true,
        });
        self.circles.push(SketchCircle {
            id: id.to_string(),
            center: pc.clone(),
            radius,
            construction,
        });
        self.constraints.push(SketchConstraint {
            id: format!("{id}_fix_center"),
            kind: SketchConstraintKind::Fixed {
                point: pc,
                x: cx,
                y: cy,
            },
        });
        self.constraints.push(SketchConstraint {
            id: format!("{id}_radius"),
            kind: SketchConstraintKind::Radius {
                target: id.to_string(),
                value: radius,
                expr: None,
            },
        });
        Ok(())
    }

    pub fn add_slot_macro(
        &mut self,
        id: &str,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        radius: f64,
    ) -> Result<(), String> {
        if radius <= 0.0 {
            return Err(format!("slot '{id}' radius must be positive"));
        }
        let dx = x2 - x1;
        let dy = y2 - y1;
        let len = (dx * dx + dy * dy).sqrt();
        let (ux, uy) = if len > 1e-9 {
            (dx / len, dy / len)
        } else {
            (1.0, 0.0)
        };
        // Normal pointing "down/right" (-normal_ccw)
        let nx = uy;
        let ny = -ux;

        // CCW loop:
        // p0 = c1 + n * r (bottom-left) -> line to p1 = c2 + n * r (bottom-right)
        // -> CCW semicircle (bulge = +1.0) to p2 = c2 - n * r (top-right)
        // -> line to p3 = c1 - n * r (top-left)
        // -> CCW semicircle (bulge = +1.0) back to p0
        let p0 = format!("{id}_p0");
        let p1 = format!("{id}_p1");
        let p2 = format!("{id}_p2");
        let p3 = format!("{id}_p3");
        let coords = [
            (p0.clone(), x1 + nx * radius, y1 + ny * radius),
            (p1.clone(), x2 + nx * radius, y2 + ny * radius),
            (p2.clone(), x2 - nx * radius, y2 - ny * radius),
            (p3.clone(), x1 - nx * radius, y1 - ny * radius),
        ];
        for (pid, px, py) in &coords {
            self.points.push(SketchPoint {
                id: pid.clone(),
                x: *px,
                y: *py,
                fixed: false,
                construction: false,
            });
            self.constraints.push(SketchConstraint {
                id: format!("{pid}_fix"),
                kind: SketchConstraintKind::Fixed {
                    point: pid.clone(),
                    x: *px,
                    y: *py,
                },
            });
        }
        self.segments.push(SketchSegment {
            id: format!("{id}_side1"),
            p1: p0.clone(),
            p2: p1.clone(),
            bulge: 0.0,
            construction: false,
        });
        self.segments.push(SketchSegment {
            id: format!("{id}_cap2"),
            p1,
            p2: p2.clone(),
            bulge: 1.0,
            construction: false,
        });
        self.segments.push(SketchSegment {
            id: format!("{id}_side2"),
            p1: p2,
            p2: p3.clone(),
            bulge: 0.0,
            construction: false,
        });
        self.segments.push(SketchSegment {
            id: format!("{id}_cap1"),
            p1: p3,
            p2: p0,
            bulge: 1.0,
            construction: false,
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_polygon_macro(
        &mut self,
        id: &str,
        cx: f64,
        cy: f64,
        radius: f64,
        sides: usize,
        angle_deg: f64,
        construction: bool,
    ) -> Result<(), String> {
        if !(3..=64).contains(&sides) {
            return Err(format!("polygon '{id}' sides must be between 3 and 64"));
        }
        if radius <= 0.0 {
            return Err(format!("polygon '{id}' radius must be positive"));
        }
        let mut pids = Vec::with_capacity(sides);
        for i in 0..sides {
            let theta =
                angle_deg.to_radians() + 2.0 * std::f64::consts::PI * (i as f64) / (sides as f64);
            let px = cx + radius * theta.cos();
            let py = cy + radius * theta.sin();
            let pid = format!("{id}_v{i}");
            self.points.push(SketchPoint {
                id: pid.clone(),
                x: px,
                y: py,
                fixed: false,
                construction,
            });
            self.constraints.push(SketchConstraint {
                id: format!("{pid}_fix"),
                kind: SketchConstraintKind::Fixed {
                    point: pid.clone(),
                    x: px,
                    y: py,
                },
            });
            pids.push(pid);
        }
        for i in 0..sides {
            let next = (i + 1) % sides;
            self.segments.push(SketchSegment {
                id: format!("{id}_e{i}"),
                p1: pids[i].clone(),
                p2: pids[next].clone(),
                bulge: 0.0,
                construction,
            });
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_bolt_circle_macro(
        &mut self,
        id: &str,
        cx: f64,
        cy: f64,
        pcd: f64,
        count: usize,
        hole_diameter: f64,
        start_angle_deg: f64,
    ) -> Result<(), String> {
        if count == 0 || pcd <= 0.0 || hole_diameter <= 0.0 {
            return Err(format!(
                "bolt_circle '{id}' requires count >= 1, pcd > 0, hole_diameter > 0"
            ));
        }
        // Construction pitch circle
        self.add_circle_macro(&format!("{id}_pcd"), cx, cy, 0.5 * pcd, true)?;
        let r_pcd = 0.5 * pcd;
        let r_hole = 0.5 * hole_diameter;
        for i in 0..count {
            let theta = start_angle_deg.to_radians()
                + 2.0 * std::f64::consts::PI * (i as f64) / (count as f64);
            let hx = cx + r_pcd * theta.cos();
            let hy = cy + r_pcd * theta.sin();
            self.add_circle_macro(&format!("{id}_h{}", i + 1), hx, hy, r_hole, false)?;
        }
        Ok(())
    }

    /// Replace a sharp corner point `point_id` joining two straight segments with a tangent
    /// circular arc of `radius`.
    pub fn fillet_sketch_corner(&mut self, point_id: &str, radius: f64) -> Result<(), String> {
        if radius <= 0.0 {
            return Err("fillet radius must be positive".into());
        }
        let corner_idx = self
            .points
            .iter()
            .position(|p| p.id == point_id)
            .ok_or_else(|| format!("corner point '{point_id}' not found in sketch"))?;
        let corner = self.points[corner_idx].clone();

        // Find incoming segment (p2 == point_id) and outgoing segment (p1 == point_id)
        let in_idx = self
            .segments
            .iter()
            .position(|s| !s.construction && s.p2 == point_id && s.bulge.abs() < 1e-9);
        let out_idx = self
            .segments
            .iter()
            .position(|s| !s.construction && s.p1 == point_id && s.bulge.abs() < 1e-9);
        let (Some(in_i), Some(out_i)) = (in_idx, out_idx) else {
            return Err(format!(
                "point '{point_id}' is not shared by an incoming and outgoing straight segment"
            ));
        };
        let p_prev_id = self.segments[in_i].p1.clone();
        let p_next_id = self.segments[out_i].p2.clone();
        let p_prev = self
            .points
            .iter()
            .find(|p| p.id == p_prev_id)
            .cloned()
            .ok_or_else(|| format!("missing point '{p_prev_id}'"))?;
        let p_next = self
            .points
            .iter()
            .find(|p| p.id == p_next_id)
            .cloned()
            .ok_or_else(|| format!("missing point '{p_next_id}'"))?;

        let v_in = [p_prev.x - corner.x, p_prev.y - corner.y];
        let v_out = [p_next.x - corner.x, p_next.y - corner.y];
        let l_in = (v_in[0] * v_in[0] + v_in[1] * v_in[1]).sqrt();
        let l_out = (v_out[0] * v_out[0] + v_out[1] * v_out[1]).sqrt();
        if l_in < 1e-6 || l_out < 1e-6 {
            return Err(format!(
                "segments incident to '{point_id}' are too short to fillet"
            ));
        }
        let u_in = [v_in[0] / l_in, v_in[1] / l_in];
        let u_out = [v_out[0] / l_out, v_out[1] / l_out];
        let cos_alpha = (u_in[0] * u_out[0] + u_in[1] * u_out[1]).clamp(-0.9999, 0.9999);
        let alpha = cos_alpha.acos(); // interior angle between the two edges
        let half_alpha = 0.5 * alpha;
        let tan_dist = radius / half_alpha.tan();
        if tan_dist >= 0.95 * l_in.min(l_out) {
            return Err(format!(
                "fillet radius {radius} mm is too large for corner '{point_id}' (max tangent trim {:.2} mm)",
                0.9 * l_in.min(l_out) * half_alpha.tan()
            ));
        }

        // Tangent points t1 (on incoming edge) and t2 (on outgoing edge)
        let t1_x = corner.x + u_in[0] * tan_dist;
        let t1_y = corner.y + u_in[1] * tan_dist;
        let t2_x = corner.x + u_out[0] * tan_dist;
        let t2_y = corner.y + u_out[1] * tan_dist;

        // Sweep angle of the arc is (pi - alpha). Sign follows orientation cross product:
        // forward direction of incoming edge is -u_in; outgoing is u_out.
        let cross = (-u_in[0]) * u_out[1] - (-u_in[1]) * u_out[0];
        let sweep = std::f64::consts::PI - alpha;
        let bulge = (0.25 * sweep).tan() * if cross >= 0.0 { 1.0 } else { -1.0 };

        let t1_id = format!("{point_id}_f1");
        let t2_id = format!("{point_id}_f2");
        // Replace `corner` point with `t1` and `t2`, and remove any constraints referencing `point_id`
        // or the two trimmed segments' lengths/fixed positions so the filleted sketch remains clean.
        let in_seg_id = self.segments[in_i].id.clone();
        let out_seg_id = self.segments[out_i].id.clone();
        self.constraints.retain(|c| match &c.kind {
            SketchConstraintKind::Fixed { point, .. } => point != point_id,
            SketchConstraintKind::Distance {
                segment: Some(s), ..
            } => s != &in_seg_id && s != &out_seg_id,
            _ => true,
        });

        self.points.remove(corner_idx);
        self.points.push(SketchPoint {
            id: t1_id.clone(),
            x: t1_x,
            y: t1_y,
            fixed: false,
            construction: false,
        });
        self.points.push(SketchPoint {
            id: t2_id.clone(),
            x: t2_x,
            y: t2_y,
            fixed: false,
            construction: false,
        });
        self.constraints.push(SketchConstraint {
            id: format!("{t1_id}_fix"),
            kind: SketchConstraintKind::Fixed {
                point: t1_id.clone(),
                x: t1_x,
                y: t1_y,
            },
        });
        self.constraints.push(SketchConstraint {
            id: format!("{t2_id}_fix"),
            kind: SketchConstraintKind::Fixed {
                point: t2_id.clone(),
                x: t2_x,
                y: t2_y,
            },
        });

        self.segments[in_i].p2 = t1_id.clone();
        self.segments[out_i].p1 = t2_id.clone();
        self.segments.insert(
            in_i + 1,
            SketchSegment {
                id: format!("{point_id}_arc"),
                p1: t1_id,
                p2: t2_id,
                bulge,
                construction: false,
            },
        );
        Ok(())
    }
}

/// Built-in engineering sketch templates for interactive exploration and MCP examples.
pub fn preset_sketch_template(name: &str) -> Result<ParametricSketch, String> {
    match name.trim().to_ascii_lowercase().as_str() {
        "flanged_mounting_bracket" | "bracket" => {
            // L-shaped parametric profile with 8 constrained points and 1 interior mounting hole
            let pts = vec![
                SketchPoint {
                    id: "p0".into(),
                    x: 0.0,
                    y: 0.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "p1".into(),
                    x: 60.0,
                    y: 0.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "p2".into(),
                    x: 60.0,
                    y: 12.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "p3".into(),
                    x: 14.0,
                    y: 12.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "p4".into(),
                    x: 14.0,
                    y: 45.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "p5".into(),
                    x: 0.0,
                    y: 45.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "hc".into(),
                    x: 42.0,
                    y: 6.0,
                    fixed: false,
                    construction: true,
                },
            ];
            let segs = vec![
                SketchSegment {
                    id: "s0".into(),
                    p1: "p0".into(),
                    p2: "p1".into(),
                    bulge: 0.0,
                    construction: false,
                },
                SketchSegment {
                    id: "s1".into(),
                    p1: "p1".into(),
                    p2: "p2".into(),
                    bulge: 0.0,
                    construction: false,
                },
                SketchSegment {
                    id: "s2".into(),
                    p1: "p2".into(),
                    p2: "p3".into(),
                    bulge: 0.0,
                    construction: false,
                },
                SketchSegment {
                    id: "s3".into(),
                    p1: "p3".into(),
                    p2: "p4".into(),
                    bulge: 0.0,
                    construction: false,
                },
                SketchSegment {
                    id: "s4".into(),
                    p1: "p4".into(),
                    p2: "p5".into(),
                    bulge: 0.0,
                    construction: false,
                },
                SketchSegment {
                    id: "s5".into(),
                    p1: "p5".into(),
                    p2: "p0".into(),
                    bulge: 0.0,
                    construction: false,
                },
            ];
            let circles = vec![SketchCircle {
                id: "hole1".into(),
                center: "hc".into(),
                radius: 3.2,
                construction: false,
            }];
            let constraints = vec![
                SketchConstraint {
                    id: "c_origin".into(),
                    kind: SketchConstraintKind::Fixed {
                        point: "p0".into(),
                        x: 0.0,
                        y: 0.0,
                    },
                },
                SketchConstraint {
                    id: "c_h0".into(),
                    kind: SketchConstraintKind::Horizontal {
                        segment: Some("s0".into()),
                        p1: None,
                        p2: None,
                    },
                },
                SketchConstraint {
                    id: "c_v1".into(),
                    kind: SketchConstraintKind::Vertical {
                        segment: Some("s1".into()),
                        p1: None,
                        p2: None,
                    },
                },
                SketchConstraint {
                    id: "c_h2".into(),
                    kind: SketchConstraintKind::Horizontal {
                        segment: Some("s2".into()),
                        p1: None,
                        p2: None,
                    },
                },
                SketchConstraint {
                    id: "c_v3".into(),
                    kind: SketchConstraintKind::Vertical {
                        segment: Some("s3".into()),
                        p1: None,
                        p2: None,
                    },
                },
                SketchConstraint {
                    id: "c_h4".into(),
                    kind: SketchConstraintKind::Horizontal {
                        segment: Some("s4".into()),
                        p1: None,
                        p2: None,
                    },
                },
                SketchConstraint {
                    id: "c_v5".into(),
                    kind: SketchConstraintKind::Vertical {
                        segment: Some("s5".into()),
                        p1: None,
                        p2: None,
                    },
                },
                SketchConstraint {
                    id: "d_base_w".into(),
                    kind: SketchConstraintKind::Distance {
                        segment: Some("s0".into()),
                        p1: None,
                        p2: None,
                        value: 60.0,
                        expr: None,
                    },
                },
                SketchConstraint {
                    id: "d_flange_t".into(),
                    kind: SketchConstraintKind::Distance {
                        segment: Some("s1".into()),
                        p1: None,
                        p2: None,
                        value: 12.0,
                        expr: None,
                    },
                },
                SketchConstraint {
                    id: "d_web_t".into(),
                    kind: SketchConstraintKind::Distance {
                        segment: Some("s4".into()),
                        p1: None,
                        p2: None,
                        value: 14.0,
                        expr: None,
                    },
                },
                SketchConstraint {
                    id: "d_total_h".into(),
                    kind: SketchConstraintKind::Distance {
                        segment: Some("s5".into()),
                        p1: None,
                        p2: None,
                        value: 45.0,
                        expr: None,
                    },
                },
                SketchConstraint {
                    id: "c_hole_pos".into(),
                    kind: SketchConstraintKind::Fixed {
                        point: "hc".into(),
                        x: 42.0,
                        y: 6.0,
                    },
                },
                SketchConstraint {
                    id: "d_hole_dia".into(),
                    kind: SketchConstraintKind::Diameter {
                        target: "hole1".into(),
                        value: 6.4,
                        expr: None,
                    },
                },
            ];
            Ok(ParametricSketch {
                id: "flanged_mounting_bracket".into(),
                plane: SketchPlane::Xy { offset: 0.0 },
                points: pts,
                segments: segs,
                circles,
                constraints,
                macros: vec![],
            })
        }
        "turned_aerospace_spool" | "spool" => {
            // Meridian profile in (r, z) for a 360-degree turned flanged bushing/spool (bore r=6..14, flanges z=0..24)
            let pts = vec![
                SketchPoint {
                    id: "p0".into(),
                    x: 6.0,
                    y: 0.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "p1".into(),
                    x: 18.0,
                    y: 0.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "p2".into(),
                    x: 18.0,
                    y: 4.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "p3".into(),
                    x: 11.0,
                    y: 4.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "p4".into(),
                    x: 11.0,
                    y: 20.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "p5".into(),
                    x: 18.0,
                    y: 20.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "p6".into(),
                    x: 18.0,
                    y: 24.0,
                    fixed: false,
                    construction: false,
                },
                SketchPoint {
                    id: "p7".into(),
                    x: 6.0,
                    y: 24.0,
                    fixed: false,
                    construction: false,
                },
            ];
            let mut segs = Vec::new();
            for i in 0..8 {
                segs.push(SketchSegment {
                    id: format!("s{i}"),
                    p1: format!("p{i}"),
                    p2: format!("p{}", (i + 1) % 8),
                    bulge: 0.0,
                    construction: false,
                });
            }
            let mut constraints = Vec::new();
            for p in &pts {
                constraints.push(SketchConstraint {
                    id: format!("fix_{}", p.id),
                    kind: SketchConstraintKind::Fixed {
                        point: p.id.clone(),
                        x: p.x,
                        y: p.y,
                    },
                });
            }
            Ok(ParametricSketch {
                id: "turned_aerospace_spool".into(),
                plane: SketchPlane::Xz { offset: 0.0 },
                points: pts,
                segments: segs,
                circles: vec![],
                constraints,
                macros: vec![],
            })
        }
        "slotted_adjustment_link" | "slot_link" => {
            let mut sk = ParametricSketch {
                id: "slotted_adjustment_link".into(),
                plane: SketchPlane::Xy { offset: 0.0 },
                points: vec![],
                segments: vec![],
                circles: vec![],
                constraints: vec![],
                macros: vec![
                    SketchMacro::Slot {
                        id: "outer_slot".into(),
                        x1: 0.0,
                        y1: 0.0,
                        x2: 50.0,
                        y2: 0.0,
                        radius: 12.0,
                    },
                    SketchMacro::Circle {
                        id: "left_bore".into(),
                        cx: 0.0,
                        cy: 0.0,
                        radius: 5.0,
                        construction: false,
                    },
                    SketchMacro::Circle {
                        id: "right_bore".into(),
                        cx: 50.0,
                        cy: 0.0,
                        radius: 5.0,
                        construction: false,
                    },
                ],
            };
            sk.expand_macros()?;
            Ok(sk)
        }
        "bolt_flange_plate" | "flange" => {
            let mut sk = ParametricSketch {
                id: "bolt_flange_plate".into(),
                plane: SketchPlane::Xy { offset: 0.0 },
                points: vec![],
                segments: vec![],
                circles: vec![],
                constraints: vec![],
                macros: vec![
                    SketchMacro::CenterRect {
                        id: "plate".into(),
                        cx: 0.0,
                        cy: 0.0,
                        width: 64.0,
                        height: 64.0,
                        fillet: 8.0,
                        construction: false,
                    },
                    SketchMacro::Circle {
                        id: "center_bore".into(),
                        cx: 0.0,
                        cy: 0.0,
                        radius: 14.0,
                        construction: false,
                    },
                    SketchMacro::BoltCircle {
                        id: "mounting_bolts".into(),
                        cx: 0.0,
                        cy: 0.0,
                        pcd: 44.0,
                        count: 4,
                        hole_diameter: 5.5,
                        start_angle_deg: 45.0,
                    },
                ],
            };
            sk.expand_macros()?;
            Ok(sk)
        }
        other => Err(format!(
            "unknown preset sketch template '{other}': expected 'flanged_mounting_bracket', 'turned_aerospace_spool', 'slotted_adjustment_link', or 'bolt_flange_plate'"
        )),
    }
}

// ---------------------------------------------------------------------------------------------
// Stage 1: Damped Levenberg–Marquardt + Homotopy Continuation + Column-Pivoted QR Solver
// ---------------------------------------------------------------------------------------------

/// Mapping between the flat state vector `x_vec` and sketch entities.
struct VarMap {
    /// Point index `i` -> `(2*i, 2*i + 1)` in `x_vec` if not `point.fixed`, else `None`.
    point_vars: Vec<Option<(usize, usize)>>,
    /// Circle index `k` -> variable index in `x_vec`.
    circle_vars: Vec<usize>,
    /// Total scalar variables `n`.
    n_vars: usize,
}

impl VarMap {
    fn build(sk: &ParametricSketch) -> Self {
        let mut n_vars = 0;
        let mut point_vars = Vec::with_capacity(sk.points.len());
        for p in &sk.points {
            if p.fixed {
                point_vars.push(None);
            } else {
                point_vars.push(Some((n_vars, n_vars + 1)));
                n_vars += 2;
            }
        }
        let mut circle_vars = Vec::with_capacity(sk.circles.len());
        for _ in &sk.circles {
            circle_vars.push(n_vars);
            n_vars += 1;
        }
        VarMap {
            point_vars,
            circle_vars,
            n_vars,
        }
    }

    fn pack(&self, sk: &ParametricSketch) -> Vec<f64> {
        let mut x = vec![0.0; self.n_vars];
        for (i, p) in sk.points.iter().enumerate() {
            if let Some((ix, iy)) = self.point_vars[i] {
                x[ix] = p.x;
                x[iy] = p.y;
            }
        }
        for (k, c) in sk.circles.iter().enumerate() {
            x[self.circle_vars[k]] = c.radius;
        }
        x
    }

    fn unpack(&self, sk: &mut ParametricSketch, x: &[f64]) {
        for (i, p) in sk.points.iter_mut().enumerate() {
            if let Some((ix, iy)) = self.point_vars[i] {
                p.x = x[ix];
                p.y = x[iy];
            }
        }
        for (k, c) in sk.circles.iter_mut().enumerate() {
            c.radius = x[self.circle_vars[k]].max(1e-6);
        }
    }
}

/// Resolve optional `.fsm` `let` expressions inside dimensional constraints before solving.
fn evaluate_constraint_expressions(
    sk: &mut ParametricSketch,
    lets: &[(String, f64)],
) -> Result<(), String> {
    for c in &mut sk.constraints {
        match &mut c.kind {
            SketchConstraintKind::Distance { value, expr, .. }
            | SketchConstraintKind::HorizontalDistance { value, expr, .. }
            | SketchConstraintKind::VerticalDistance { value, expr, .. }
            | SketchConstraintKind::Radius { value, expr, .. }
            | SketchConstraintKind::Diameter { value, expr, .. } => {
                if let Some(ex) = expr.as_deref() {
                    if !ex.trim().is_empty() {
                        *value = eval_expr(ex, lets).map_err(|e| {
                            format!("constraint '{}' expression '{}': {}", c.id, ex, e)
                        })?;
                    }
                }
            }
            SketchConstraintKind::Angle { degrees, expr, .. } => {
                if let Some(ex) = expr.as_deref() {
                    if !ex.trim().is_empty() {
                        *degrees = eval_expr(ex, lets).map_err(|e| {
                            format!("constraint '{}' expression '{}': {}", c.id, ex, e)
                        })?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Helper to look up `(x, y)` of a point by id from `(sk, x_vec, vmap)`.
fn pt_xy(sk: &ParametricSketch, x: &[f64], vmap: &VarMap, id: &str) -> Result<[f64; 2], String> {
    let idx = sk
        .points
        .iter()
        .position(|p| p.id == id)
        .ok_or_else(|| format!("unknown sketch point '{id}'"))?;
    if let Some((ix, iy)) = vmap.point_vars[idx] {
        Ok([x[ix], x[iy]])
    } else {
        Ok([sk.points[idx].x, sk.points[idx].y])
    }
}

/// Helper to look up the endpoint pair `([x1, y1], [x2, y2])` of either a named `segment` or `(p1, p2)`.
fn resolve_pair_xy(
    sk: &ParametricSketch,
    x: &[f64],
    vmap: &VarMap,
    segment: Option<&str>,
    p1: Option<&str>,
    p2: Option<&str>,
) -> Result<([f64; 2], [f64; 2]), String> {
    if let Some(seg_id) = segment {
        let seg = sk
            .segments
            .iter()
            .find(|s| s.id == seg_id)
            .ok_or_else(|| format!("unknown sketch segment '{seg_id}'"))?;
        Ok((
            pt_xy(sk, x, vmap, &seg.p1)?,
            pt_xy(sk, x, vmap, &seg.p2)?,
        ))
    } else if let (Some(a), Some(b)) = (p1, p2) {
        Ok((pt_xy(sk, x, vmap, a)?, pt_xy(sk, x, vmap, b)?))
    } else {
        Err("constraint requires either 'segment' or both 'p1' and 'p2'".into())
    }
}

/// Compute center `[cx, cy]` and radius `R` of a circle or circular arc segment.
fn circle_or_arc_geom(
    sk: &ParametricSketch,
    x: &[f64],
    vmap: &VarMap,
    target_id: &str,
) -> Result<([f64; 2], f64), String> {
    if let Some(c_idx) = sk.circles.iter().position(|c| c.id == target_id) {
        let c = &sk.circles[c_idx];
        let center = pt_xy(sk, x, vmap, &c.center)?;
        let r = x[vmap.circle_vars[c_idx]].abs();
        return Ok((center, r));
    }
    if let Some(seg) = sk.segments.iter().find(|s| s.id == target_id) {
        let a = pt_xy(sk, x, vmap, &seg.p1)?;
        let b = pt_xy(sk, x, vmap, &seg.p2)?;
        let k = seg.bulge;
        if k.abs() < 1e-9 {
            return Err(format!(
                "segment '{target_id}' has bulge=0 (straight line), not a circular arc"
            ));
        }
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let chord = (dx * dx + dy * dy).sqrt().max(1e-12);
        let r = chord * (1.0 + k * k) / (4.0 * k.abs());
        let ux = dx / chord;
        let uy = dy / chord;
        let nx = -uy;
        let ny = ux;
        let offset = chord * (1.0 - k * k) / (4.0 * k);
        let cx = 0.5 * (a[0] + b[0]) + nx * offset;
        let cy = 0.5 * (a[1] + b[1]) + ny * offset;
        return Ok(([cx, cy], r));
    }
    Err(format!(
        "target '{target_id}' is neither a circle nor an arc segment"
    ))
}

/// Evaluate all scalar constraint equations `r(x)`, returning `(residuals, row_to_constraint_id)`.
/// All residuals are scaled to units of millimeters (angle & direction residuals are multiplied
/// by characteristic chord length so Jacobian conditioning is uniform across length and angle rows).
fn eval_residuals(
    sk: &ParametricSketch,
    x: &[f64],
    vmap: &VarMap,
    blend_alpha: f64,
    initial_x: &[f64],
) -> Result<(Vec<f64>, Vec<String>), String> {
    let mut r = Vec::with_capacity(sk.constraints.len() * 2);
    let mut row_ids = Vec::with_capacity(sk.constraints.len() * 2);

    // Helper for homotopy blending of target dimensional values from initial_x (when alpha < 1.0)
    let blend = |target_val: f64, init_val: f64| -> f64 {
        if blend_alpha >= 0.999999 {
            target_val
        } else {
            init_val + blend_alpha * (target_val - init_val)
        }
    };

    for c in &sk.constraints {
        match &c.kind {
            SketchConstraintKind::Coincident { p1, p2 } => {
                let a = pt_xy(sk, x, vmap, p1)?;
                let b = pt_xy(sk, x, vmap, p2)?;
                r.push(a[0] - b[0]);
                row_ids.push(c.id.clone());
                r.push(a[1] - b[1]);
                row_ids.push(c.id.clone());
            }
            SketchConstraintKind::Fixed { point, x: tx, y: ty } => {
                let p = pt_xy(sk, x, vmap, point)?;
                let p0 = pt_xy(sk, initial_x, vmap, point)?;
                r.push(p[0] - blend(*tx, p0[0]));
                row_ids.push(c.id.clone());
                r.push(p[1] - blend(*ty, p0[1]));
                row_ids.push(c.id.clone());
            }
            SketchConstraintKind::Horizontal { segment, p1, p2 } => {
                let (a, b) =
                    resolve_pair_xy(sk, x, vmap, segment.as_deref(), p1.as_deref(), p2.as_deref())?;
                r.push(b[1] - a[1]);
                row_ids.push(c.id.clone());
            }
            SketchConstraintKind::Vertical { segment, p1, p2 } => {
                let (a, b) =
                    resolve_pair_xy(sk, x, vmap, segment.as_deref(), p1.as_deref(), p2.as_deref())?;
                r.push(b[0] - a[0]);
                row_ids.push(c.id.clone());
            }
            SketchConstraintKind::Parallel { seg1, seg2 } => {
                let (a1, b1) = resolve_pair_xy(sk, x, vmap, Some(seg1), None, None)?;
                let (a2, b2) = resolve_pair_xy(sk, x, vmap, Some(seg2), None, None)?;
                let d1 = [b1[0] - a1[0], b1[1] - a1[1]];
                let d2 = [b2[0] - a2[0], b2[1] - a2[1]];
                let l2 = (d2[0] * d2[0] + d2[1] * d2[1]).sqrt().max(1e-9);
                // Cross product scaled by 1/l2 so residual has mm units:
                r.push((d1[0] * d2[1] - d1[1] * d2[0]) / l2);
                row_ids.push(c.id.clone());
            }
            SketchConstraintKind::Perpendicular { seg1, seg2 } => {
                let (a1, b1) = resolve_pair_xy(sk, x, vmap, Some(seg1), None, None)?;
                let (a2, b2) = resolve_pair_xy(sk, x, vmap, Some(seg2), None, None)?;
                let d1 = [b1[0] - a1[0], b1[1] - a1[1]];
                let d2 = [b2[0] - a2[0], b2[1] - a2[1]];
                let l2 = (d2[0] * d2[0] + d2[1] * d2[1]).sqrt().max(1e-9);
                // Dot product scaled by 1/l2 so residual has mm units:
                r.push((d1[0] * d2[0] + d1[1] * d2[1]) / l2);
                row_ids.push(c.id.clone());
            }
            SketchConstraintKind::EqualLength { seg1, seg2 } => {
                // Support either two segments or two circles
                if sk.circles.iter().any(|ci| &ci.id == seg1)
                    && sk.circles.iter().any(|ci| &ci.id == seg2)
                {
                    let (_, r1) = circle_or_arc_geom(sk, x, vmap, seg1)?;
                    let (_, r2) = circle_or_arc_geom(sk, x, vmap, seg2)?;
                    r.push(r1 - r2);
                    row_ids.push(c.id.clone());
                } else {
                    let (a1, b1) = resolve_pair_xy(sk, x, vmap, Some(seg1), None, None)?;
                    let (a2, b2) = resolve_pair_xy(sk, x, vmap, Some(seg2), None, None)?;
                    let l1 = ((b1[0] - a1[0]).powi(2) + (b1[1] - a1[1]).powi(2)).sqrt();
                    let l2 = ((b2[0] - a2[0]).powi(2) + (b2[1] - a2[1]).powi(2)).sqrt();
                    r.push(l1 - l2);
                    row_ids.push(c.id.clone());
                }
            }
            SketchConstraintKind::Tangent { seg, target } => {
                let (a, b) = resolve_pair_xy(sk, x, vmap, Some(seg), None, None)?;
                let (center, rad) = circle_or_arc_geom(sk, x, vmap, target)?;
                let dx = b[0] - a[0];
                let dy = b[1] - a[1];
                let len = (dx * dx + dy * dy).sqrt().max(1e-9);
                // Signed perpendicular distance from circle center to line (a, b)
                let signed_dist = ((center[0] - a[0]) * (-dy) + (center[1] - a[1]) * dx) / len;
                r.push(signed_dist.abs() - rad);
                row_ids.push(c.id.clone());
            }
            SketchConstraintKind::Concentric { c1, c2 } => {
                let (center1, _) = circle_or_arc_geom(sk, x, vmap, c1)?;
                let (center2, _) = circle_or_arc_geom(sk, x, vmap, c2)?;
                r.push(center1[0] - center2[0]);
                row_ids.push(c.id.clone());
                r.push(center1[1] - center2[1]);
                row_ids.push(c.id.clone());
            }
            SketchConstraintKind::Midpoint {
                point,
                segment,
                p1,
                p2,
            } => {
                let pm = pt_xy(sk, x, vmap, point)?;
                let (a, b) =
                    resolve_pair_xy(sk, x, vmap, segment.as_deref(), p1.as_deref(), p2.as_deref())?;
                r.push(pm[0] - 0.5 * (a[0] + b[0]));
                row_ids.push(c.id.clone());
                r.push(pm[1] - 0.5 * (a[1] + b[1]));
                row_ids.push(c.id.clone());
            }
            SketchConstraintKind::Symmetric { p1, p2, axis } => {
                let a = pt_xy(sk, x, vmap, p1)?;
                let b = pt_xy(sk, x, vmap, p2)?;
                match axis.to_ascii_lowercase().as_str() {
                    "y_axis" | "y" | "vertical" => {
                        // Symmetric across x = 0 (Y axis): x1 + x2 = 0, y1 - y2 = 0
                        r.push(a[0] + b[0]);
                        row_ids.push(c.id.clone());
                        r.push(a[1] - b[1]);
                        row_ids.push(c.id.clone());
                    }
                    "x_axis" | "x" | "horizontal" => {
                        // Symmetric across y = 0 (X axis): x1 - x2 = 0, y1 + y2 = 0
                        r.push(a[0] - b[0]);
                        row_ids.push(c.id.clone());
                        r.push(a[1] + b[1]);
                        row_ids.push(c.id.clone());
                    }
                    seg_id => {
                        let (l1, l2) = resolve_pair_xy(sk, x, vmap, Some(seg_id), None, None)?;
                        let dx = l2[0] - l1[0];
                        let dy = l2[1] - l1[1];
                        let len = (dx * dx + dy * dy).sqrt().max(1e-9);
                        let ux = dx / len;
                        let uy = dy / len;
                        // 1. Midpoint lies on the axis line: (mid - l1) x u = 0
                        let mx = 0.5 * (a[0] + b[0]) - l1[0];
                        let my = 0.5 * (a[1] + b[1]) - l1[1];
                        r.push(mx * uy - my * ux);
                        row_ids.push(c.id.clone());
                        // 2. Chord (b - a) is perpendicular to axis u: (b - a) . u = 0
                        r.push((b[0] - a[0]) * ux + (b[1] - a[1]) * uy);
                        row_ids.push(c.id.clone());
                    }
                }
            }
            SketchConstraintKind::PointOnLine { point, segment } => {
                let p = pt_xy(sk, x, vmap, point)?;
                let (a, b) = resolve_pair_xy(sk, x, vmap, Some(segment), None, None)?;
                let dx = b[0] - a[0];
                let dy = b[1] - a[1];
                let len = (dx * dx + dy * dy).sqrt().max(1e-9);
                r.push(((p[0] - a[0]) * dy - (p[1] - a[1]) * dx) / len);
                row_ids.push(c.id.clone());
            }
            SketchConstraintKind::Distance {
                segment,
                p1,
                p2,
                value,
                ..
            } => {
                let (a, b) =
                    resolve_pair_xy(sk, x, vmap, segment.as_deref(), p1.as_deref(), p2.as_deref())?;
                let (a0, b0) = resolve_pair_xy(
                    sk,
                    initial_x,
                    vmap,
                    segment.as_deref(),
                    p1.as_deref(),
                    p2.as_deref(),
                )?;
                let dist = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
                let dist0 = ((b0[0] - a0[0]).powi(2) + (b0[1] - a0[1]).powi(2)).sqrt();
                r.push(dist - blend(*value, dist0));
                row_ids.push(c.id.clone());
            }
            SketchConstraintKind::HorizontalDistance {
                segment,
                p1,
                p2,
                value,
                ..
            } => {
                let (a, b) =
                    resolve_pair_xy(sk, x, vmap, segment.as_deref(), p1.as_deref(), p2.as_deref())?;
                let (a0, b0) = resolve_pair_xy(
                    sk,
                    initial_x,
                    vmap,
                    segment.as_deref(),
                    p1.as_deref(),
                    p2.as_deref(),
                )?;
                let sign = if b0[0] >= a0[0] { 1.0 } else { -1.0 };
                let dx = (b[0] - a[0]) * sign;
                let dx0 = (b0[0] - a0[0]).abs();
                r.push(dx - blend(*value, dx0));
                row_ids.push(c.id.clone());
            }
            SketchConstraintKind::VerticalDistance {
                segment,
                p1,
                p2,
                value,
                ..
            } => {
                let (a, b) =
                    resolve_pair_xy(sk, x, vmap, segment.as_deref(), p1.as_deref(), p2.as_deref())?;
                let (a0, b0) = resolve_pair_xy(
                    sk,
                    initial_x,
                    vmap,
                    segment.as_deref(),
                    p1.as_deref(),
                    p2.as_deref(),
                )?;
                let sign = if b0[1] >= a0[1] { 1.0 } else { -1.0 };
                let dy = (b[1] - a[1]) * sign;
                let dy0 = (b0[1] - a0[1]).abs();
                r.push(dy - blend(*value, dy0));
                row_ids.push(c.id.clone());
            }
            SketchConstraintKind::Angle {
                seg1,
                seg2,
                degrees,
                ..
            } => {
                let (a1, b1) = resolve_pair_xy(sk, x, vmap, Some(seg1), None, None)?;
                let d1 = [b1[0] - a1[0], b1[1] - a1[1]];
                let l1 = (d1[0] * d1[0] + d1[1] * d1[1]).sqrt().max(1.0);
                let ang1 = d1[1].atan2(d1[0]);
                let ang_actual = if let Some(s2) = seg2.as_deref() {
                    let (a2, b2) = resolve_pair_xy(sk, x, vmap, Some(s2), None, None)?;
                    let d2 = [b2[0] - a2[0], b2[1] - a2[1]];
                    let ang2 = d2[1].atan2(d2[0]);
                    ang2 - ang1
                } else {
                    ang1
                };
                let target_rad = degrees.to_radians();
                // Wrap angle difference into (-pi, pi] and scale by chord length l1
                let diff = (ang_actual - target_rad).sin().atan2((ang_actual - target_rad).cos());
                r.push(diff * l1);
                row_ids.push(c.id.clone());
            }
            SketchConstraintKind::Radius { target, value, .. } => {
                let (_, rad) = circle_or_arc_geom(sk, x, vmap, target)?;
                let (_, rad0) = circle_or_arc_geom(sk, initial_x, vmap, target)?;
                r.push(rad - blend(*value, rad0));
                row_ids.push(c.id.clone());
            }
            SketchConstraintKind::Diameter { target, value, .. } => {
                let (_, rad) = circle_or_arc_geom(sk, x, vmap, target)?;
                let (_, rad0) = circle_or_arc_geom(sk, initial_x, vmap, target)?;
                r.push(2.0 * rad - blend(*value, 2.0 * rad0));
                row_ids.push(c.id.clone());
            }
        }
    }

    Ok((r, row_ids))
}

/// Compute `m × n` Jacobian matrix `J` via central differences (`h = 1e-7`).
fn compute_jacobian(
    sk: &ParametricSketch,
    x: &[f64],
    vmap: &VarMap,
    blend_alpha: f64,
    initial_x: &[f64],
    m: usize,
) -> Result<Vec<f64>, String> {
    let n = vmap.n_vars;
    let mut j = vec![0.0; m * n];
    if m == 0 || n == 0 {
        return Ok(j);
    }
    let h = 1e-7;
    let mut x_work = x.to_vec();
    for col in 0..n {
        let orig = x_work[col];
        x_work[col] = orig + h;
        let (rp, _) = eval_residuals(sk, &x_work, vmap, blend_alpha, initial_x)?;
        x_work[col] = orig - h;
        let (rm, _) = eval_residuals(sk, &x_work, vmap, blend_alpha, initial_x)?;
        x_work[col] = orig;
        let inv_2h = 0.5 / h;
        for row in 0..m {
            j[row * n + col] = (rp[row] - rm[row]) * inv_2h;
        }
    }
    Ok(j)
}

/// Solve symmetric positive-definite linear system `A * dx = b` (`n × n`) using Cholesky / Gaussian
/// elimination with partial pivoting.
fn solve_linear_system(mut a: Vec<f64>, mut b: Vec<f64>, n: usize) -> Option<Vec<f64>> {
    for k in 0..n {
        let mut piv_row = k;
        let mut max_val = a[k * n + k].abs();
        for i in (k + 1)..n {
            let v = a[i * n + k].abs();
            if v > max_val {
                max_val = v;
                piv_row = i;
            }
        }
        if max_val < 1e-14 {
            return None;
        }
        if piv_row != k {
            for col in k..n {
                a.swap(k * n + col, piv_row * n + col);
            }
            b.swap(k, piv_row);
        }
        let diag = a[k * n + k];
        for i in (k + 1)..n {
            let factor = a[i * n + k] / diag;
            a[i * n + k] = 0.0;
            for col in (k + 1)..n {
                a[i * n + col] -= factor * a[k * n + col];
            }
            b[i] -= factor * b[k];
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let mut s = b[i];
        for j in (i + 1)..n {
            s -= a[i * n + j] * x[j];
        }
        x[i] = s / a[i * n + i];
    }
    Some(x)
}

/// Result of row-pivoted & column-pivoted rank analysis on `J` (`m × n`).
struct RankAnalysis {
    rank: usize,
    dependent_rows: Vec<usize>,
    constrained_cols: BTreeSet<usize>,
}

/// Perform Gaussian elimination with complete pivoting on `J` (`m × n`) to determine:
/// 1. Numerical rank `rank(J)` (`pivot > 1e-5`).
/// 2. Dependent rows (`row_perm[rank..m]`), which identify redundant or conflicting constraints.
/// 3. Which variable columns are actively constrained vs free in the nullspace.
fn analyze_jacobian_rank(j_mat: &[f64], m: usize, n: usize) -> RankAnalysis {
    if m == 0 || n == 0 {
        return RankAnalysis {
            rank: 0,
            dependent_rows: (0..m).collect(),
            constrained_cols: BTreeSet::new(),
        };
    }
    let mut a = j_mat.to_vec();
    let mut row_perm: Vec<usize> = (0..m).collect();
    let mut col_perm: Vec<usize> = (0..n).collect();
    let max_k = m.min(n);
    let mut rank = 0;

    for k in 0..max_k {
        let mut best_val = 0.0;
        let mut best_r = k;
        let mut best_c = k;
        for r in k..m {
            for c in k..n {
                let v = a[r * n + c].abs();
                if v > best_val {
                    best_val = v;
                    best_r = r;
                    best_c = c;
                }
            }
        }
        if best_val < 1e-5 {
            break;
        }
        if best_r != k {
            for c in 0..n {
                a.swap(k * n + c, best_r * n + c);
            }
            row_perm.swap(k, best_r);
        }
        if best_c != k {
            for r in 0..m {
                a.swap(r * n + k, r * n + best_c);
            }
            col_perm.swap(k, best_c);
        }
        let piv = a[k * n + k];
        for r in (k + 1)..m {
            let f = a[r * n + k] / piv;
            a[r * n + k] = 0.0;
            for c in (k + 1)..n {
                a[r * n + c] -= f * a[k * n + c];
            }
        }
        rank += 1;
    }

    // Back-substitute upper-triangular R (rank x n) into Reduced Row Echelon Form (RREF)
    // so we can see exactly which columns have zero coupling to the free columns `rank..n`.
    for k in (0..rank).rev() {
        let piv = a[k * n + k];
        for c in k..n {
            a[k * n + c] /= piv;
        }
        for r in 0..k {
            let f = a[r * n + k];
            a[r * n + k] = 0.0;
            for c in k..n {
                a[r * n + c] -= f * a[k * n + c];
            }
        }
    }

    // A pivot column `col_perm[k]` (`k < rank`) is uniquely locked (`DOF = 0`) iff its RREF row `k`
    // has zero entries in all free columns `c in rank..n`.
    let mut constrained_cols = BTreeSet::new();
    for k in 0..rank {
        let mut coupled_to_free = false;
        for c in rank..n {
            if a[k * n + c].abs() > 1e-4 {
                coupled_to_free = true;
                break;
            }
        }
        if !coupled_to_free {
            constrained_cols.insert(col_perm[k]);
        }
    }

    let dependent_rows = row_perm[rank..m].to_vec();
    RankAnalysis {
        rank,
        dependent_rows,
        constrained_cols,
    }
}

/// Compile solved non-construction segments and circles into closed `ProfileVertex` loops,
/// classifying the largest-area loop (in CCW orientation) as `outer_profile` and any interior
/// loops as `hole_profiles`.
pub fn compile_sketch_profiles(
    sk: &ParametricSketch,
) -> (Vec<ProfileVertex>, Vec<Vec<ProfileVertex>>) {
    let pt_map: BTreeMap<&str, [f64; 2]> = sk
        .points
        .iter()
        .map(|p| (p.id.as_str(), [p.x, p.y]))
        .collect();

    let mut loops: Vec<Vec<ProfileVertex>> = Vec::new();

    // 1. Chain non-construction segments into closed polygonal/arc loops
    let active_segs: Vec<&SketchSegment> =
        sk.segments.iter().filter(|s| !s.construction).collect();
    let mut visited = vec![false; active_segs.len()];

    for start_idx in 0..active_segs.len() {
        if visited[start_idx] {
            continue;
        }
        visited[start_idx] = true;
        let first = active_segs[start_idx];
        let Some(&p1_xy) = pt_map.get(first.p1.as_str()) else {
            continue;
        };
        let Some(&p2_xy) = pt_map.get(first.p2.as_str()) else {
            continue;
        };
        let mut cur_xy = p2_xy;
        let mut chain = vec![ProfileVertex {
            x: p1_xy[0],
            y: p1_xy[1],
            bulge: first.bulge,
        }];
        let start_xy = p1_xy;
        let mut cur_id = first.p2.as_str();

        loop {
            if ((cur_xy[0] - start_xy[0]).powi(2) + (cur_xy[1] - start_xy[1]).powi(2)).sqrt()
                < 1e-4
                && chain.len() >= 2
            {
                loops.push(ensure_ccw_loop(chain));
                break;
            }
            // Find next unvisited segment connected to cur_id (or within 1e-4 mm of cur_xy)
            let mut found_next = None;
            for (idx, seg) in active_segs.iter().enumerate() {
                if visited[idx] {
                    continue;
                }
                let Some(&sa) = pt_map.get(seg.p1.as_str()) else {
                    continue;
                };
                let Some(&sb) = pt_map.get(seg.p2.as_str()) else {
                    continue;
                };
                if seg.p1 == cur_id
                    || ((sa[0] - cur_xy[0]).powi(2) + (sa[1] - cur_xy[1]).powi(2)).sqrt() < 1e-4
                {
                    found_next = Some((idx, sa, sb, seg.p2.as_str(), seg.bulge));
                    break;
                } else if seg.p2 == cur_id
                    || ((sb[0] - cur_xy[0]).powi(2) + (sb[1] - cur_xy[1]).powi(2)).sqrt() < 1e-4
                {
                    // Reversed segment
                    found_next = Some((idx, sb, sa, seg.p1.as_str(), -seg.bulge));
                    break;
                }
            }
            if let Some((idx, seg_start, seg_end, next_id, bulge)) = found_next {
                visited[idx] = true;
                chain.push(ProfileVertex {
                    x: seg_start[0],
                    y: seg_start[1],
                    bulge,
                });
                cur_xy = seg_end;
                cur_id = next_id;
            } else {
                break;
            }
        }
    }

    // 2. Convert non-construction circles into 2-vertex semicircle loops (`bulge = 1.0`)
    for c in &sk.circles {
        if c.construction {
            continue;
        }
        if let Some(&center) = pt_map.get(c.center.as_str()) {
            let r = c.radius.abs().max(1e-6);
            loops.push(vec![
                ProfileVertex {
                    x: center[0] - r,
                    y: center[1],
                    bulge: 1.0,
                },
                ProfileVertex {
                    x: center[0] + r,
                    y: center[1],
                    bulge: 1.0,
                },
            ]);
        }
    }

    if loops.is_empty() {
        return (vec![], vec![]);
    }

    // Sort loops by absolute enclosed area descending: largest is `outer_profile`, rest are `hole_profiles`
    loops.sort_by(|a, b| {
        loop_abs_area(b)
            .partial_cmp(&loop_abs_area(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let outer = loops.remove(0);
    (outer, loops)
}

fn signed_loop_area(verts: &[ProfileVertex]) -> f64 {
    let n = verts.len();
    if n < 2 {
        return 0.0;
    }
    let mut area = 0.0;
    for i in 0..n {
        let a = &verts[i];
        let b = &verts[(i + 1) % n];
        // Polygon shoelace term
        area += 0.5 * (a.x * b.y - b.x * a.y);
        // Circular segment area contribution if bulge != 0
        if a.bulge.abs() > 1e-9 {
            let dx = b.x - a.x;
            let dy = b.y - a.y;
            let c2 = dx * dx + dy * dy;
            let theta = 4.0 * a.bulge.atan();
            let r2 = c2 * (1.0 + a.bulge * a.bulge).powi(2) / (16.0 * a.bulge * a.bulge);
            area += 0.5 * r2 * (theta - theta.sin());
        }
    }
    area
}

fn loop_abs_area(verts: &[ProfileVertex]) -> f64 {
    signed_loop_area(verts).abs()
}

fn ensure_ccw_loop(mut verts: Vec<ProfileVertex>) -> Vec<ProfileVertex> {
    if signed_loop_area(&verts) >= 0.0 {
        return verts;
    }
    // Reverse loop order and negate bulges (shifting bulge from edge i to reversed edge)
    let n = verts.len();
    let mut rev = Vec::with_capacity(n);
    for i in 0..n {
        let curr = &verts[(n - 1 - i + 1) % n];
        let prev = &verts[n - 1 - i];
        rev.push(ProfileVertex {
            x: curr.x,
            y: curr.y,
            bulge: -prev.bulge,
        });
    }
    verts = rev;
    verts
}

/// Solve a `ParametricSketch` using homotopy continuation + damped Levenberg–Marquardt
/// and compute its exact Jacobian rank, DOF, and constraint diagnostics.
pub fn solve_sketch(
    sketch: &mut ParametricSketch,
    lets: &[(String, f64)],
) -> Result<SketchSolveReport, String> {
    sketch.expand_macros()?;
    evaluate_constraint_expressions(sketch, lets)?;

    let vmap = VarMap::build(sketch);
    let initial_x = vmap.pack(sketch);
    let mut x = initial_x.clone();
    let n = vmap.n_vars;

    // Check initial residual at target (alpha = 1.0)
    let (r_init, row_ids) = eval_residuals(sketch, &x, &vmap, 1.0, &initial_x)?;
    let m = r_init.len();
    let init_max_r = r_init.iter().fold(0.0_f64, |acc, v| acc.max(v.abs()));

    // Homotopy continuation schedule: if initial residual is large (> 5.0 mm), step alpha in 4 stages
    let alphas: &[f64] = if init_max_r > 5.0 && m > 0 && n > 0 {
        &[0.25, 0.5, 0.75, 1.0]
    } else {
        &[1.0]
    };

    let mut total_iters = 0;
    for &alpha in alphas {
        let mut lambda = 1e-4_f64;
        for _iter in 0..60 {
            let (r, _) = eval_residuals(sketch, &x, &vmap, alpha, &initial_x)?;
            let max_r = r.iter().fold(0.0_f64, |acc, v| acc.max(v.abs()));
            if max_r <= 1e-9 || m == 0 || n == 0 {
                break;
            }
            total_iters += 1;
            let cost: f64 = r.iter().map(|v| v * v).sum();
            let j = compute_jacobian(sketch, &x, &vmap, alpha, &initial_x, m)?;

            // Build normal equations (J^T J + lambda * I) dx = -J^T r
            let mut jtj = vec![0.0; n * n];
            let mut jtr = vec![0.0; n];
            for row in 0..m {
                let rv = r[row];
                for c1 in 0..n {
                    let j1 = j[row * n + c1];
                    jtr[c1] -= j1 * rv;
                    for c2 in c1..n {
                        jtj[c1 * n + c2] += j1 * j[row * n + c2];
                    }
                }
            }
            for c1 in 0..n {
                for c2 in 0..c1 {
                    jtj[c1 * n + c2] = jtj[c2 * n + c1];
                }
                jtj[c1 * n + c1] += lambda;
            }

            if let Some(dx) = solve_linear_system(jtj, jtr, n) {
                let step_norm: f64 = dx.iter().map(|v| v * v).sum::<f64>().sqrt();
                // Trust-region cap per step so homotopy never jumps wildly
                let scale = if step_norm > 25.0 {
                    25.0 / step_norm
                } else {
                    1.0
                };
                let x_cand: Vec<f64> = x
                    .iter()
                    .zip(dx.iter())
                    .map(|(xi, dxi)| xi + scale * dxi)
                    .collect();
                let (r_cand, _) = eval_residuals(sketch, &x_cand, &vmap, alpha, &initial_x)?;
                let cost_cand: f64 = r_cand.iter().map(|v| v * v).sum();
                if cost_cand < cost {
                    x = x_cand;
                    lambda = (lambda * 0.3).max(1e-9);
                    if step_norm < 1e-9 {
                        break;
                    }
                } else {
                    lambda = (lambda * 5.0).min(1e6);
                }
            } else {
                lambda = (lambda * 10.0).min(1e6);
            }
        }
    }

    vmap.unpack(sketch, &x);
    let (final_r, _) = eval_residuals(sketch, &x, &vmap, 1.0, &initial_x)?;
    let max_residual = final_r.iter().fold(0.0_f64, |acc, v| acc.max(v.abs()));
    let l2_residual = final_r.iter().map(|v| v * v).sum::<f64>().sqrt();
    let final_j = compute_jacobian(sketch, &x, &vmap, 1.0, &initial_x, m)?;
    let rank_info = analyze_jacobian_rank(&final_j, m, n);
    let dof = n.saturating_sub(rank_info.rank);

    // Classify redundant vs conflicting constraints
    let mut redundant_set = BTreeSet::new();
    let mut conflict_set = BTreeSet::new();
    for (row_idx, &res) in final_r.iter().enumerate() {
        if res.abs() > 1e-4 {
            conflict_set.insert(row_ids[row_idx].clone());
        }
    }
    for &dep_row in &rank_info.dependent_rows {
        let cid = &row_ids[dep_row];
        if final_r[dep_row].abs() <= 1e-4 && !conflict_set.contains(cid) {
            redundant_set.insert(cid.clone());
        }
    }

    let redundant_constraints: Vec<String> = redundant_set.into_iter().collect();
    let conflicting_constraints: Vec<String> = conflict_set.into_iter().collect();
    let converged = conflicting_constraints.is_empty();

    let status = if !conflicting_constraints.is_empty() {
        SketchStatus::OverConstrainedConflict
    } else if !redundant_constraints.is_empty() {
        SketchStatus::RedundantConstraints
    } else if dof == 0 {
        SketchStatus::FullyConstrained
    } else {
        SketchStatus::UnderConstrained
    };

    let mut point_freedom = Vec::with_capacity(sketch.points.len());
    for (i, p) in sketch.points.iter().enumerate() {
        let p_dof = if let Some((ix, iy)) = vmap.point_vars[i] {
            let fx = usize::from(!rank_info.constrained_cols.contains(&ix));
            let fy = usize::from(!rank_info.constrained_cols.contains(&iy));
            fx + fy
        } else {
            0
        };
        let p_status = if !conflicting_constraints.is_empty() {
            SketchStatus::OverConstrainedConflict
        } else if p_dof == 0 {
            SketchStatus::FullyConstrained
        } else {
            SketchStatus::UnderConstrained
        };
        point_freedom.push(PointFreedom {
            id: p.id.clone(),
            x: p.x,
            y: p.y,
            dof: p_dof,
            status: p_status,
        });
    }

    let diagnosis = match status {
        SketchStatus::FullyConstrained => format!(
            "Sketch '{}' is fully constrained (DOF = 0, rank = {}/{}, max residual = {:.2e} mm).",
            sketch.id, rank_info.rank, n, max_residual
        ),
        SketchStatus::UnderConstrained => {
            let free_pts: Vec<String> = point_freedom
                .iter()
                .filter(|pf| pf.dof > 0)
                .map(|pf| format!("{} ({} DOF)", pf.id, pf.dof))
                .collect();
            format!(
                "Sketch '{}' is under-constrained with {} DOF remaining (rank = {}/{}). Free points: [{}].",
                sketch.id,
                dof,
                rank_info.rank,
                n,
                free_pts.join(", ")
            )
        }
        SketchStatus::RedundantConstraints => format!(
            "Sketch '{}' converged (DOF = {}), but has {} redundant constraint(s): [{}]. Remove one to keep the Jacobian full-rank.",
            sketch.id,
            dof,
            redundant_constraints.len(),
            redundant_constraints.join(", ")
        ),
        SketchStatus::OverConstrainedConflict => format!(
            "Sketch '{}' is over-constrained with conflicting constraint(s): [{}] (max residual = {:.4} mm). Remove or relax one of the conflicting constraints.",
            sketch.id,
            conflicting_constraints.join(", "),
            max_residual
        ),
    };

    let (outer_profile, hole_profiles) = compile_sketch_profiles(sketch);

    Ok(SketchSolveReport {
        status,
        converged,
        iterations: total_iters,
        max_residual,
        l2_residual,
        total_variables: n,
        total_equations: m,
        jacobian_rank: rank_info.rank,
        dof,
        redundant_constraints,
        conflicting_constraints,
        diagnosis,
        point_freedom,
        outer_profile,
        hole_profiles,
        solved_sketch: sketch.clone(),
    })
}

// ---------------------------------------------------------------------------------------------
// Stage 2: Surface-Attached 3D Feature Engine (`BossExtrude`, `PocketCut`, `Revolve`, `HolePattern`)
// ---------------------------------------------------------------------------------------------

/// 3D modelling operation built from a solved `ParametricSketch`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchFeatureOp {
    /// Add or union an extruded boss of `height` (mm) normal to the sketch plane/face,
    /// automatically subtracting any interior `hole_profiles`.
    BossExtrude {
        height: f64,
        #[serde(default)]
        blend: f64,
        #[serde(default)]
        symmetric: bool,
    },
    /// Subtract a blind (`depth > 0`) or through-all (`through_all = true`) pocket into the
    /// host solid face (or along `-normal` of the sketch plane).
    PocketCut {
        #[serde(default = "default_pocket_depth")]
        depth: f64,
        #[serde(default)]
        through_all: bool,
        #[serde(default)]
        blend: f64,
    },
    /// Revolve the solved 2D profile around the sketch `Z` (meridian) axis by `angle_deg` (`1..=360`).
    RevolveBoss {
        #[serde(default = "default_revolve_angle")]
        angle_deg: f64,
        #[serde(default)]
        blend: f64,
    },
    /// Subtract a revolved groove or bore around the sketch `Z` axis by `angle_deg` (`1..=360`).
    RevolveCut {
        #[serde(default = "default_revolve_angle")]
        angle_deg: f64,
        #[serde(default)]
        blend: f64,
    },
}

fn default_pocket_depth() -> f64 {
    10.0
}

fn default_revolve_angle() -> f64 {
    360.0
}

/// Request payload for `apply_sketch_feature_to_document` (and the `apply_sketch_feature` MCP tool).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchFeatureRequest {
    /// ID of the new 3D feature node to create or update in the `.fsm` document.
    pub feature_id: String,
    /// The parametric sketch (or preset template name via `preset`).
    #[serde(default)]
    pub sketch: ParametricSketch,
    /// Optional preset template name (`"flanged_mounting_bracket"`, `"turned_aerospace_spool"`, etc.).
    #[serde(default)]
    pub preset: Option<String>,
    /// Optional shorthand plane/face override (`"xy"`, `"xz"`, `"yz"`, or `"<host_id>.<face>"`).
    #[serde(default)]
    pub on: Option<String>,
    /// 3D feature operation (`boss_extrude`, `pocket_cut`, `revolve_boss`, `revolve_cut`).
    pub operation: SketchFeatureOp,
    /// Optional named `.fsm` `let` parameters to upsert before solving the sketch.
    #[serde(default)]
    pub lets: BTreeMap<String, f64>,
}

/// Summary returned after compiling a `SketchFeatureRequest` into a `Document`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchFeatureSummary {
    pub feature_id: String,
    pub created_node_ids: Vec<String>,
    pub plane_description: String,
    pub world_origin: [f64; 3],
    pub world_rotation_deg: [f64; 3],
    pub solve_report: SketchSolveReport,
}

/// Resolved 3D frame for placing an extruded or revolved sketch feature in world coordinates.
#[derive(Debug, Clone)]
pub struct ResolvedSketchFrame {
    /// Center of the target face or datum plane in world `(x, y, z)`.
    pub plane_origin: [f64; 3],
    /// Outward unit normal of the sketch plane/face in world coordinates.
    pub outward_normal: [f64; 3],
    /// Euler angles `rotate = (rx, ry, rz)` in degrees that map local `+Z` to `outward_normal`.
    pub rotate_deg: [f64; 3],
    /// Thickness of the host solid along `-outward_normal` (used for `through_all` pocket cuts).
    pub host_thickness: f64,
    pub description: String,
}

/// Resolve a `SketchPlane` against a `Document`'s shapes to determine its 3D origin,
/// outward unit normal, and Euler rotation.
pub fn resolve_sketch_plane_frame(
    doc: &Document,
    plane: &SketchPlane,
) -> Result<ResolvedSketchFrame, String> {
    match plane {
        SketchPlane::Xy { offset } => Ok(ResolvedSketchFrame {
            plane_origin: [0.0, 0.0, *offset],
            outward_normal: [0.0, 0.0, 1.0],
            rotate_deg: [0.0, 0.0, 0.0],
            host_thickness: 50.0,
            description: format!("World XY plane (z = {offset:.2} mm, normal +Z)"),
        }),
        SketchPlane::Xz { offset } => Ok(ResolvedSketchFrame {
            plane_origin: [0.0, *offset, 0.0],
            outward_normal: [0.0, -1.0, 0.0],
            rotate_deg: [90.0, 0.0, 0.0],
            host_thickness: 50.0,
            description: format!("World XZ plane (y = {offset:.2} mm)"),
        }),
        SketchPlane::Yz { offset } => Ok(ResolvedSketchFrame {
            plane_origin: [*offset, 0.0, 0.0],
            outward_normal: [1.0, 0.0, 0.0],
            rotate_deg: [0.0, 90.0, 0.0],
            host_thickness: 50.0,
            description: format!("World YZ plane (x = {offset:.2} mm, normal +X)"),
        }),
        SketchPlane::OnFace {
            host_id,
            face,
            offset,
        } => {
            let node = find_node_recursive(&doc.root, host_id).ok_or_else(|| {
                format!("sketch attachment host shape '{host_id}' not found in document")
            })?;
            let at = node.transform.translate;
            let (hx, hy, hz) = node_half_extents(&node.kind);
            let (n, dist, thick) = match face {
                Face::Top | Face::Pz => ([0.0, 0.0, 1.0], hz, 2.0 * hz),
                Face::Bottom | Face::Nz => ([0.0, 0.0, -1.0], hz, 2.0 * hz),
                Face::Px => ([1.0, 0.0, 0.0], hx, 2.0 * hx),
                Face::Nx => ([-1.0, 0.0, 0.0], hx, 2.0 * hx),
                Face::Py => ([0.0, 1.0, 0.0], hy, 2.0 * hy),
                Face::Ny => ([0.0, -1.0, 0.0], hy, 2.0 * hy),
                Face::Side => ([1.0, 0.0, 0.0], hx, 2.0 * hx),
                Face::All => ([0.0, 0.0, 1.0], hz, 2.0 * hz),
            };
            let d = dist + offset;
            let origin = [at[0] + n[0] * d, at[1] + n[1] * d, at[2] + n[2] * d];
            let rotate_deg = match face {
                Face::Top | Face::Pz | Face::All => [0.0, 0.0, 0.0],
                Face::Bottom | Face::Nz => [180.0, 0.0, 0.0],
                Face::Px | Face::Side => [0.0, 90.0, 0.0],
                Face::Nx => [0.0, -90.0, 0.0],
                Face::Py => [-90.0, 0.0, 0.0],
                Face::Ny => [90.0, 0.0, 0.0],
            };
            Ok(ResolvedSketchFrame {
                plane_origin: origin,
                outward_normal: n,
                rotate_deg,
                host_thickness: thick.max(10.0),
                description: format!(
                    "Surface '{}.{}' at ({:.2}, {:.2}, {:.2}) mm",
                    host_id,
                    face.name(),
                    origin[0],
                    origin[1],
                    origin[2]
                ),
            })
        }
    }
}

fn find_node_recursive<'a>(nodes: &'a [Node], id: &str) -> Option<&'a Node> {
    for n in nodes {
        if n.id == id {
            return Some(n);
        }
        if let NodeKind::Group { children, .. } = &n.kind {
            if let Some(found) = find_node_recursive(children, id) {
                return Some(found);
            }
        }
    }
    None
}

fn node_half_extents(kind: &NodeKind) -> (f64, f64, f64) {
    match kind {
        NodeKind::Box { size, .. } => (0.5 * size[0], 0.5 * size[1], 0.5 * size[2]),
        NodeKind::Cylinder { radius, height, .. } => (*radius, *radius, 0.5 * height),
        NodeKind::Sphere { radius } => (*radius, *radius, *radius),
        NodeKind::Capsule { radius, length } => (*radius, *radius, 0.5 * length + radius),
        NodeKind::Torus { major, minor } => (major + minor, major + minor, *minor),
        NodeKind::Extrude {
            profile, height, ..
        } => {
            let mut rx = 10.0_f64;
            let mut ry = 10.0_f64;
            for v in profile {
                rx = rx.max(v.x.abs());
                ry = ry.max(v.y.abs());
            }
            (rx, ry, 0.5 * height)
        }
        NodeKind::Revolve { profile, .. } => {
            let mut r_max = 10.0_f64;
            let mut z_max = 10.0_f64;
            for v in profile {
                r_max = r_max.max(v.x.abs());
                z_max = z_max.max(v.y.abs());
            }
            (r_max, r_max, z_max)
        }
        _ => (20.0, 20.0, 10.0),
    }
}

fn upsert_node(doc: &mut Document, node: Node) {
    if let Some(existing) = doc.root.iter_mut().find(|n| n.id == node.id) {
        *existing = node;
    } else {
        doc.root.push(node);
    }
}

/// Solve a `SketchFeatureRequest` and compile it into 3D `.fsm` nodes on `doc`.
pub fn apply_sketch_feature_to_document(
    doc: &mut Document,
    req: &SketchFeatureRequest,
) -> Result<SketchFeatureSummary, String> {
    if req.feature_id.trim().is_empty() {
        return Err("feature_id must not be empty".into());
    }

    // 1. Upsert any requested `.fsm` `let` bindings
    for (k, v) in &req.lets {
        if let Some(existing) = doc.lets.iter_mut().find(|l| &l.name == k) {
            existing.value = *v;
            existing.expr = format!("{v}");
        } else {
            doc.lets.push(Let {
                name: k.clone(),
                value: *v,
                expr: format!("{v}"),
            });
        }
    }
    let let_pairs: Vec<(String, f64)> = doc
        .lets
        .iter()
        .map(|l| (l.name.clone(), l.value))
        .collect();

    // 2. Load preset sketch if requested, and apply `on` plane override
    let mut sketch = if let Some(preset_name) = req.preset.as_deref() {
        let mut p = preset_sketch_template(preset_name)?;
        if !req.sketch.constraints.is_empty() {
            // Allow caller to append or override constraints on top of a preset
            for c in &req.sketch.constraints {
                if let Some(ex) = p.constraints.iter_mut().find(|ec| ec.id == c.id) {
                    *ex = c.clone();
                } else {
                    p.constraints.push(c.clone());
                }
            }
        }
        p
    } else {
        req.sketch.clone()
    };

    if let Some(on_spec) = req.on.as_deref() {
        if !on_spec.trim().is_empty() {
            sketch.plane = SketchPlane::parse_spec(on_spec, 0.0)?;
        }
    }

    // 3. Solve the 2D parametric sketch
    let report = solve_sketch(&mut sketch, &let_pairs)?;
    if report.outer_profile.len() < 2 {
        return Err(format!(
            "sketch '{}' did not produce a closed non-construction profile loop (got {} vertices)",
            sketch.id,
            report.outer_profile.len()
        ));
    }

    // 4. Resolve 3D coordinate frame from `sketch.plane`
    let frame = resolve_sketch_plane_frame(doc, &sketch.plane)?;
    let n = frame.outward_normal;
    let mut created_ids = Vec::new();
    let mut world_origin = frame.plane_origin;

    match &req.operation {
        SketchFeatureOp::BossExtrude {
            height,
            blend,
            symmetric,
        } => {
            if *height <= 0.0 {
                return Err("boss_extrude height must be positive".into());
            }
            // `Prim::Extrude` spans `z in [-height/2, +height/2]` in its local frame.
            // Shifting along `+outward_normal` by `height/2` seats the boss bottom flush on the face!
            let shift = if *symmetric { 0.0 } else { 0.5 * *height };
            world_origin = [
                frame.plane_origin[0] + n[0] * shift,
                frame.plane_origin[1] + n[1] * shift,
                frame.plane_origin[2] + n[2] * shift,
            ];
            upsert_node(
                doc,
                Node {
                    id: req.feature_id.clone(),
                    kind: NodeKind::Extrude {
                        profile: report.outer_profile.clone(),
                        height: *height,
                    },
                    op: Op::Add,
                    blend: *blend,
                    suppressed: false,
                    transform: Transform {
                        translate: world_origin,
                        rotate: frame.rotate_deg,
                    },
                    material: None,
                    color: None,
                },
            );
            created_ids.push(req.feature_id.clone());

            // Automatically subtract any interior hole loops through the boss!
            for (idx, hole_loop) in report.hole_profiles.iter().enumerate() {
                let hole_id = format!("{}_hole{}", req.feature_id, idx + 1);
                upsert_node(
                    doc,
                    Node {
                        id: hole_id.clone(),
                        kind: NodeKind::Extrude {
                            profile: hole_loop.clone(),
                            height: *height + 1.0,
                        },
                        op: Op::Subtract,
                        blend: 0.0,
                        suppressed: false,
                        transform: Transform {
                            translate: world_origin,
                            rotate: frame.rotate_deg,
                        },
                        material: None,
                        color: None,
                    },
                );
                created_ids.push(hole_id);
            }
        }
        SketchFeatureOp::PocketCut {
            depth,
            through_all,
            blend,
        } => {
            let cut_depth = if *through_all {
                frame.host_thickness + 2.0
            } else {
                (*depth).max(0.1)
            };
            // Shift inward (`-outward_normal`) by `cut_depth/2 - 0.25` so top of tool clears the surface by 0.5 mm
            let cut_h = cut_depth + 0.5;
            let shift = -0.5 * cut_depth + 0.25;
            world_origin = [
                frame.plane_origin[0] + n[0] * shift,
                frame.plane_origin[1] + n[1] * shift,
                frame.plane_origin[2] + n[2] * shift,
            ];
            upsert_node(
                doc,
                Node {
                    id: req.feature_id.clone(),
                    kind: NodeKind::Extrude {
                        profile: report.outer_profile.clone(),
                        height: cut_h,
                    },
                    op: Op::Subtract,
                    blend: *blend,
                    suppressed: false,
                    transform: Transform {
                        translate: world_origin,
                        rotate: frame.rotate_deg,
                    },
                    material: None,
                    color: None,
                },
            );
            created_ids.push(req.feature_id.clone());
        }
        SketchFeatureOp::RevolveBoss { angle_deg, blend } => {
            upsert_node(
                doc,
                Node {
                    id: req.feature_id.clone(),
                    kind: NodeKind::Revolve {
                        profile: report.outer_profile.clone(),
                        angle: angle_deg.clamp(1.0, 360.0),
                    },
                    op: Op::Add,
                    blend: *blend,
                    suppressed: false,
                    transform: Transform {
                        translate: world_origin,
                        rotate: [0.0, 0.0, 0.0],
                    },
                    material: None,
                    color: None,
                },
            );
            created_ids.push(req.feature_id.clone());
        }
        SketchFeatureOp::RevolveCut { angle_deg, blend } => {
            upsert_node(
                doc,
                Node {
                    id: req.feature_id.clone(),
                    kind: NodeKind::Revolve {
                        profile: report.outer_profile.clone(),
                        angle: angle_deg.clamp(1.0, 360.0),
                    },
                    op: Op::Subtract,
                    blend: *blend,
                    suppressed: false,
                    transform: Transform {
                        translate: world_origin,
                        rotate: [0.0, 0.0, 0.0],
                    },
                    material: None,
                    color: None,
                },
            );
            created_ids.push(req.feature_id.clone());
        }
    }

    Ok(SketchFeatureSummary {
        feature_id: req.feature_id.clone(),
        created_node_ids: created_ids,
        plane_description: frame.description,
        world_origin,
        world_rotation_deg: frame.rotate_deg,
        solve_report: report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_constraints_solve_with_zero_dof() {
        let mut sk = ParametricSketch {
            id: "rect_test".into(),
            plane: SketchPlane::Xy { offset: 0.0 },
            points: vec![],
            segments: vec![],
            circles: vec![],
            constraints: vec![],
            macros: vec![SketchMacro::Rect {
                id: "r1".into(),
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 25.0,
                fillet: 0.0,
                construction: false,
            }],
        };
        let rep = solve_sketch(&mut sk, &[]).expect("solve rect");
        assert_eq!(rep.status, SketchStatus::FullyConstrained);
        assert_eq!(rep.dof, 0);
        assert!(rep.max_residual < 1e-8);
        assert_eq!(rep.outer_profile.len(), 4);
    }

    #[test]
    fn redundant_and_conflicting_constraints_are_diagnosed() {
        let mut sk = ParametricSketch::default();
        sk.add_rect_macro("r", 0.0, 0.0, 30.0, 20.0, 0.0, false)
            .unwrap();
        // Add a redundant width constraint on the top edge with the SAME value 30.0
        sk.constraints.push(SketchConstraint {
            id: "redundant_top_width".into(),
            kind: SketchConstraintKind::Distance {
                segment: Some("r_top".into()),
                p1: None,
                p2: None,
                value: 30.0,
                expr: None,
            },
        });
        let rep_red = solve_sketch(&mut sk, &[]).unwrap();
        assert_eq!(rep_red.status, SketchStatus::RedundantConstraints);
        assert!(!rep_red.redundant_constraints.is_empty());

        // Now change that constraint to 45.0 so it conflicts with the bottom width = 30.0 + vertical sides
        if let Some(c) = sk
            .constraints
            .iter_mut()
            .find(|c| c.id == "redundant_top_width")
        {
            c.kind = SketchConstraintKind::Distance {
                segment: Some("r_top".into()),
                p1: None,
                p2: None,
                value: 45.0,
                expr: None,
            };
        }
        let rep_conf = solve_sketch(&mut sk, &[]).unwrap();
        assert_eq!(rep_conf.status, SketchStatus::OverConstrainedConflict);
        assert!(!rep_conf.conflicting_constraints.is_empty());
    }
}
