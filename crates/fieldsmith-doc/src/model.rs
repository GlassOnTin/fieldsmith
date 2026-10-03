//! The Fieldsmith document model.
//!
//! Units are fixed: millimetres, newtons, megapascals, tonnes, seconds (a self-consistent
//! system: 1 N = 1 t·mm/s², 1 MPa = 1 N/mm²). Display units are a UI concern only.

use serde::{Deserialize, Serialize};

/// A complete Fieldsmith document: geometry, analysis setup, fill and output settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub name: String,
    /// The model: an ordered group of shapes. The part is whatever the root group evaluates to.
    pub root: Vec<Node>,
    pub regions: Vec<Region>,
    pub supports: Vec<Support>,
    pub loads: Vec<Load>,
    /// Material id from the library (see [`crate::materials`]) or a custom material id.
    pub material: String,
    pub custom_materials: Vec<crate::materials::Material>,
    pub fill: Option<Fill>,
    pub solve: SolveSettings,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            name: "Untitled".into(),
            root: Vec::new(),
            regions: Vec::new(),
            supports: Vec::new(),
            loads: Vec::new(),
            material: "petg".into(),
            custom_materials: Vec::new(),
            fill: None,
            solve: SolveSettings::default(),
        }
    }
}

/// How a node combines with everything before it in its group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    #[default]
    Add,
    Subtract,
    Intersect,
}

/// Rigid placement: `world = R · local + translate`, with `R = Rz · Ry · Rx` (degrees).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Transform {
    pub translate: [f64; 3],
    pub rotate: [f64; 3],
}

impl Transform {
    pub fn is_identity(&self) -> bool {
        self.translate == [0.0; 3] && self.rotate == [0.0; 3]
    }
}

/// A node in the model tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Unique, stable identifier (also the display name). `[A-Za-z_][A-Za-z0-9_]*`.
    pub id: String,
    pub op: Op,
    /// Blend (fillet) radius in mm used when combining with the preceding nodes. 0 = sharp.
    pub blend: f64,
    pub suppressed: bool,
    pub transform: Transform,
    pub kind: NodeKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum NodeKind {
    /// Centred box. `round` rounds all edges.
    Box {
        size: [f64; 3],
        round: f64,
    },
    /// Cylinder along local Z, centred. `round` rounds both rims.
    Cylinder {
        radius: f64,
        height: f64,
        round: f64,
    },
    Sphere {
        radius: f64,
    },
    /// Capsule along local Z; `length` is the distance between the two cap centres.
    Capsule {
        radius: f64,
        length: f64,
    },
    /// Torus in the local XY plane.
    Torus {
        major: f64,
        minor: f64,
    },
    /// Closed 2D profile in local XY, extruded symmetrically along Z.
    Extrude {
        profile: Vec<ProfileVertex>,
        height: f64,
    },
    Group {
        children: Vec<Node>,
        modifiers: Vec<Modifier>,
    },
}

/// A profile vertex. `bulge` describes the segment *leaving* this vertex:
/// 0 = straight line, otherwise `tan(θ/4)` of the arc's included angle (positive = counter-clockwise).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileVertex {
    pub x: f64,
    pub y: f64,
    pub bulge: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Axis::X => "x",
            Axis::Y => "y",
            Axis::Z => "z",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShellSide {
    Inside,
    Outside,
    Centre,
}

/// Modifiers apply, in order, to a group's combined result in the group's local frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Modifier {
    Shell {
        thickness: f64,
        side: ShellSide,
    },
    Offset {
        distance: f64,
    },
    /// Keeps the original and adds its reflection in the plane `axis = 0`.
    Mirror {
        axis: Axis,
    },
    /// `count` copies, each displaced by `step` from the previous one.
    Linear {
        count: u32,
        step: [f64; 3],
    },
    /// `count` copies rotated about `axis` through the origin. `angle` = 360 spreads evenly.
    Circular {
        count: u32,
        axis: Axis,
        angle: f64,
    },
}

/// Faces a region selector can refer to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Face {
    /// Every surface of the shape.
    All,
    Px,
    Nx,
    Py,
    Ny,
    Pz,
    Nz,
    /// Cylindrical / profile side wall.
    Side,
    Top,
    Bottom,
}

impl Face {
    pub const ALL: [Face; 10] = [
        Face::All,
        Face::Px,
        Face::Nx,
        Face::Py,
        Face::Ny,
        Face::Pz,
        Face::Nz,
        Face::Side,
        Face::Top,
        Face::Bottom,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Face::All => "all",
            Face::Px => "px",
            Face::Nx => "nx",
            Face::Py => "py",
            Face::Ny => "ny",
            Face::Pz => "pz",
            Face::Nz => "nz",
            Face::Side => "side",
            Face::Top => "top",
            Face::Bottom => "bottom",
        }
    }
    pub fn parse(s: &str) -> Option<Face> {
        Face::ALL.iter().copied().find(|f| f.name() == s)
    }
}

/// One face (or all faces) of one shape. Regions are unions of selectors.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Selector {
    pub shape: String,
    pub face: Face,
}

/// A named portion of the part's surface, defined as a field (surface of a shape's face).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub id: String,
    pub selectors: Vec<Selector>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Support {
    pub id: String,
    pub region: String,
    /// Which displacement components are held at zero. `[true; 3]` = fully fixed.
    pub fixed: [bool; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum LoadKind {
    /// Total force in N, distributed uniformly over the region's area.
    Force { vector: [f64; 3] },
    /// Pressure in MPa acting against the outward normal (positive pushes into the part).
    Pressure { mpa: f64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Load {
    pub id: String,
    pub region: String,
    /// Load case name. Loads sharing a case are applied together.
    pub case: String,
    pub kind: LoadKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Gyroid,
    Primitive,
    Diamond,
}

impl Family {
    pub fn name(self) -> &'static str {
        match self {
            Family::Gyroid => "gyroid",
            Family::Primitive => "primitive",
            Family::Diamond => "diamond",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FillMode {
    /// Thin double-sided wall following the minimal surface.
    Sheet,
    /// Solid struts on one side of the minimal surface.
    Network,
}

/// Interior lattice fill.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fill {
    pub family: Family,
    pub mode: FillMode,
    /// Unit cell edge length (mm).
    pub cell: f64,
    /// Solid skin thickness kept under the outer surface (mm).
    pub skin: f64,
    /// Relative density bounds of the lattice region (0..1).
    pub rho_min: f64,
    pub rho_max: f64,
    /// Grading exponent: ρ = ρmin + (ρmax − ρmin)·(σ/σref)^curve.
    pub curve: f64,
    /// Thinnest printable wall / strut (mm); densities are raised to respect it.
    pub min_wall: f64,
    /// Shapes kept solid within `margin` of their surface (e.g. holes).
    pub protect: Vec<String>,
    pub margin: f64,
    /// Load case that drives the grading; `None` uses the envelope over all cases.
    pub case: Option<String>,
}

impl Default for Fill {
    fn default() -> Self {
        Self {
            family: Family::Gyroid,
            mode: FillMode::Sheet,
            cell: 6.0,
            skin: 1.2,
            rho_min: 0.2,
            rho_max: 0.8,
            curve: 1.0,
            min_wall: 0.8,
            protect: Vec::new(),
            margin: 2.0,
            case: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SolveSettings {
    /// Target element size for the finest analysis level (mm). `None` = automatic.
    pub size: Option<f64>,
}

impl Document {
    /// Find a node anywhere in the tree by id.
    pub fn find(&self, id: &str) -> Option<&Node> {
        fn rec<'a>(nodes: &'a [Node], id: &str) -> Option<&'a Node> {
            for n in nodes {
                if n.id == id {
                    return Some(n);
                }
                if let NodeKind::Group { children, .. } = &n.kind {
                    if let Some(f) = rec(children, id) {
                        return Some(f);
                    }
                }
            }
            None
        }
        rec(&self.root, id)
    }

    /// All node ids in tree order.
    pub fn node_ids(&self) -> Vec<String> {
        fn rec(nodes: &[Node], out: &mut Vec<String>) {
            for n in nodes {
                out.push(n.id.clone());
                if let NodeKind::Group { children, .. } = &n.kind {
                    rec(children, out);
                }
            }
        }
        let mut out = Vec::new();
        rec(&self.root, &mut out);
        out
    }

    /// Load case names in first-appearance order.
    pub fn load_cases(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for l in &self.loads {
            if !out.contains(&l.case) {
                out.push(l.case.clone());
            }
        }
        out
    }

    /// Validate references and values. Returns human-readable problems (empty = valid).
    pub fn validate(&self) -> Vec<String> {
        let mut errs = Vec::new();
        let ids = self.node_ids();
        let mut seen = std::collections::HashSet::new();
        for id in &ids {
            if !crate::fsm::is_ident(id) {
                errs.push(format!(
                    "'{id}' is not a valid name (letters, digits, _; not starting with a digit)"
                ));
            }
            if !seen.insert(id.clone()) {
                errs.push(format!("Name '{id}' is used more than once"));
            }
        }
        let region_ids: Vec<&str> = self.regions.iter().map(|r| r.id.as_str()).collect();
        for r in &self.regions {
            if !seen.insert(r.id.clone()) {
                errs.push(format!("Name '{}' is used more than once", r.id));
            }
            if r.selectors.is_empty() {
                errs.push(format!("Region '{}' selects nothing", r.id));
            }
            for s in &r.selectors {
                if !ids.contains(&s.shape) {
                    errs.push(format!(
                        "Region '{}' refers to missing shape '{}'",
                        r.id, s.shape
                    ));
                }
            }
        }
        for s in &self.supports {
            if !region_ids.contains(&s.region.as_str()) {
                errs.push(format!(
                    "Support '{}' refers to missing region '{}'",
                    s.id, s.region
                ));
            }
            if !s.fixed.iter().any(|&f| f) {
                errs.push(format!("Support '{}' holds no direction", s.id));
            }
        }
        for l in &self.loads {
            if !region_ids.contains(&l.region.as_str()) {
                errs.push(format!(
                    "Load '{}' refers to missing region '{}'",
                    l.id, l.region
                ));
            }
        }
        if crate::materials::find(self, &self.material).is_none() {
            errs.push(format!("Unknown material '{}'", self.material));
        }
        if let Some(f) = &self.fill {
            if f.cell <= 0.0 {
                errs.push("Fill cell size must be positive".into());
            }
            if !(0.0 < f.rho_min && f.rho_min <= f.rho_max && f.rho_max <= 1.0) {
                errs.push("Fill densities must satisfy 0 < ρmin ≤ ρmax ≤ 1".into());
            }
            for p in &f.protect {
                if !ids.contains(p) {
                    errs.push(format!("Fill protects missing shape '{p}'"));
                }
            }
        }
        errs
    }

    /// A fresh unique id with the given prefix (`box1`, `box2`, …).
    pub fn fresh_id(&self, prefix: &str) -> String {
        let mut taken: std::collections::HashSet<String> = self.node_ids().into_iter().collect();
        taken.extend(self.regions.iter().map(|r| r.id.clone()));
        taken.extend(self.supports.iter().map(|r| r.id.clone()));
        taken.extend(self.loads.iter().map(|r| r.id.clone()));
        (1..)
            .map(|i| format!("{prefix}{i}"))
            .find(|c| !taken.contains(c))
            .unwrap()
    }
}
