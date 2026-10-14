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
    /// Fixed boundary temperatures (°C), placed like loads: one region, one value,
    /// one thermal case. See [`Temperature`].
    #[serde(default)]
    pub temperatures: Vec<Temperature>,
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
            temperatures: Vec::new(),
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
    /// This volume's material id when the document assigns it one (`material <id> on
    /// <node>`); `None` takes the document material. Labelling only: the solver evaluates
    /// every volume with the document material (analysis stays single-material), and the
    /// labels ride the 3MF export to the slicer. `validate` refuses unknown ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// This volume's render colour as canonical `#RRGGBB` or `#RRGGBBAA` (`color <hex> on
    /// <node>`). Display colour only — the 3MF spec says the same of its `displaycolor`: it
    /// never affects the solve or what is printed. `validate` refuses malformed hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub kind: NodeKind,
}

/// Canonicalise a text colour: optional `#`, then exactly 6 or 8 hex digits, stored
/// uppercase with the `#`. `None` when the input is anything else.
pub fn canonical_color(input: &str) -> Option<String> {
    let digits = input.strip_prefix('#').unwrap_or(input);
    if !(digits.len() == 6 || digits.len() == 8) || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", digits.to_ascii_uppercase()))
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
    /// A voxel signed-distance field imported from an external mesh (GLB / STL / OBJ). The
    /// heavy grid travels in the `.fsmz` sidecar keyed by `hash`; the document keeps only
    /// provenance and the summary of the import report. Units and up-axis were baked into
    /// the grid at import, so the node carries no transform of its own beyond `transform`.
    Mesh {
        /// Source file path as imported (provenance only; never reloaded from).
        src: String,
        /// hex fnv1a64 of the source mesh content + baked placement; matches the sidecar
        /// entry `dist`/`patch` are loaded from.
        hash: String,
        /// Interpretation of the source's raw units: `mm`, `cm`, `m` or `in`.
        units: String,
        /// Up-axis correction applied to the source coordinates: `y` (rotate +90° about X)
        /// or `z` (none).
        up: String,
        /// Requested voxel spacing in mm at import; the baked grid may be coarser if the
        /// cell budget required it.
        voxel: f64,
        /// Baked axis-aligned bounds in mm: `[xmin, ymin, zmin, xmax, ymax, zmax]`.
        bbox: [f64; 6],
        /// Import-report summary.
        tris: u32,
        patches: u32,
        open_edges: u32,
    },
    /// Region-driven extension of an imported mesh (goal M6): the cells of the named
    /// region's mesh-patch selectors are swept along `dir` for `len` millimetres as one
    /// solid, which combines with its siblings through this node's `op`. The swept grid is
    /// compiled into a synthetic mesh field keyed by an input hash of the parent mesh and
    /// these parameters; the node stores nothing about it beyond this record.
    Extend {
        /// Region whose mesh-patch selectors feed the sweep.
        region: String,
        /// Sweep direction in mm (length is separate, so this need not be unit length).
        dir: [f64; 3],
        /// Sweep length in mm along `dir`, measured along its unit direction.
        len: f64,
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
    /// The axis a face's outward normals point along (unit; `None` for `All` and `Side`).
    /// On imported meshes an axis face is a normal-direction rule: every surface whose
    /// outward normal lies within 45° of the axis (cos ≥ 0.7, the same cone the field
    /// crate's sweep mask and `region_facets` share) belongs to the face. Primitives keep
    /// their exact face semantics.
    pub fn axis(self) -> Option<[f64; 3]> {
        match self {
            Face::Px => Some([1.0, 0.0, 0.0]),
            Face::Nx => Some([-1.0, 0.0, 0.0]),
            Face::Py => Some([0.0, 1.0, 0.0]),
            Face::Ny => Some([0.0, -1.0, 0.0]),
            Face::Pz | Face::Top => Some([0.0, 0.0, 1.0]),
            Face::Nz | Face::Bottom => Some([0.0, 0.0, -1.0]),
            Face::All | Face::Side => None,
        }
    }
}

/// One face (or all faces) of one shape. Regions are unions of selectors.
/// `patch` selects a surface patch of an imported mesh (`mesh1.p3`) instead of a geometric
/// face; when present `shape` must name a mesh node and `face` is ignored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Selector {
    pub shape: String,
    pub face: Face,
    /// Surface-patch id of the mesh node (segmentation ids start at 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<u32>,
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
    /// Force per unit area (N/mm²) from a compiled `fn` over the region's face-local
    /// coordinates, applied in `dir`. `total` optionally rescales the function so the
    /// applied resultant equals that many newtons (signed against the stated direction).
    Distributed {
        expr: String,
        dir: LoadDir,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        total: Option<f64>,
    },
}

/// The direction a distributed load pushes in: a fixed global vector, or components
/// in the region's face-local frame (x, y, then the outward normal n).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "lowercase")]
pub enum LoadDir {
    Global { v: [f64; 3] },
    Local { v: [f64; 3] },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Load {
    pub id: String,
    pub region: String,
    /// Load case name. Loads sharing a case are applied together.
    pub case: String,
    pub kind: LoadKind,
}

/// A fixed boundary temperature (°C) on a labelled region for one thermal case, placed
/// the way a load is. Dirichlet in the conduction solve: every P2 node on the region's
/// facets takes the value; faces without a temperature are insulated (the natural
/// zero-flux condition). Two temperatures claiming the same region in one case cannot
/// both be carried by a shared node — `validate` refuses that pair.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Temperature {
    pub id: String,
    pub region: String,
    /// Fixed boundary temperature (°C).
    pub celsius: f64,
    /// Thermal case name. Temperatures sharing a case are solved together, then drive
    /// thermal expansion from the solve's reference temperature.
    pub case: String,
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
    /// When set, an analysis also solves for this many modal (natural-frequency) modes.
    /// `None` = static solves only. The frequencies run on the same mesh as the statics
    /// (M8): one meshing serves both.
    pub modes: Option<usize>,
    /// Reference temperature (°C) the thermal strain measures from: ε_th = α·(T − t_ref).
    /// Thermal stress on a fully free part is zero exactly at the reference temperature.
    /// The statement's unit is °C like the temperature values. M14.
    #[serde(default)]
    pub t_ref: Option<f64>,
}

impl Document {
    /// Find a node anywhere in the tree by id.
    /// Assign `mat` as the material label of the node `target` (searched recursively).
    /// False when the document declares no shape with that id.
    pub fn set_node_material(&mut self, target: &str, mat: &str) -> bool {
        fn rec(nodes: &mut [Node], target: &str, mat: &str) -> bool {
            for n in nodes {
                if n.id == target {
                    n.material = Some(mat.to_string());
                    return true;
                }
                if let NodeKind::Group { children, .. } = &mut n.kind {
                    if rec(children, target, mat) {
                        return true;
                    }
                }
            }
            false
        }
        rec(&mut self.root, target, mat)
    }

    /// Every non-suppressed node's material label, document order: `(node id, material)`.
    pub fn material_labels(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        Self::for_each_labelled(&self.root, &mut |n, m| {
            if !n.suppressed {
                out.push((n.id.clone(), m.to_string()));
            }
        });
        out
    }

    /// Labelling warnings for the solve and export: per-volume labels change nothing in
    /// the solve (analysis stays single-material) and only travel with the 3MF export.
    pub fn material_warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mixed: Vec<String> = self
            .material_labels()
            .iter()
            .filter(|(_, m)| *m != self.material)
            .map(|(id, m)| format!("'{id}' ({m})"))
            .collect();
        if !mixed.is_empty() {
            out.push(format!(
                "Volumes {} carry a different material label; the solve evaluates all of them with '{}'. Labels travel with the 3MF export.",
                mixed.join(", "),
                self.material
            ));
        }
        out
    }

    /// Visit `(node, its material id)` for every labelled node, groups recursed, including
    /// suppressed ones (`validate` checks the label id even where the node is inactive).
    fn for_each_labelled<'a>(nodes: &'a [Node], f: &mut impl FnMut(&'a Node, &'a str)) {
        for n in nodes {
            if let Some(m) = &n.material {
                f(n, m);
            }
            if let NodeKind::Group { children, .. } = &n.kind {
                Self::for_each_labelled(children, f);
            }
        }
    }

    /// Assign `col` as the render colour of the node `target` (searched recursively);
    /// `None` clears it. False when the document declares no shape with that id.
    pub fn set_node_color(&mut self, target: &str, col: Option<String>) -> bool {
        fn rec(nodes: &mut [Node], target: &str, col: Option<&str>) -> bool {
            for n in nodes {
                if n.id == target {
                    n.color = col.map(|s| s.to_string());
                    return true;
                }
                if let NodeKind::Group { children, .. } = &mut n.kind {
                    if rec(children, target, col) {
                        return true;
                    }
                }
            }
            false
        }
        rec(&mut self.root, target, col.as_deref())
    }

    /// Visit `(node, its colour)` for every coloured node, groups recursed, including
    /// suppressed ones (`validate` checks the hex even where the node is inactive).
    fn for_each_coloured<'a>(nodes: &'a [Node], f: &mut impl FnMut(&'a Node, &'a str)) {
        for n in nodes {
            if let Some(c) = &n.color {
                f(n, c);
            }
            if let NodeKind::Group { children, .. } = &n.kind {
                Self::for_each_coloured(children, f);
            }
        }
    }

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

    /// Mutable `find`: the parameter sweep (M16) edits one number of one node in place.
    pub fn find_mut(&mut self, id: &str) -> Option<&mut Node> {
        fn rec<'a>(nodes: &'a mut [Node], id: &str) -> Option<&'a mut Node> {
            for n in nodes {
                if n.id == id {
                    return Some(n);
                }
                if let NodeKind::Group { children, .. } = &mut n.kind {
                    if let Some(f) = rec(children, id) {
                        return Some(f);
                    }
                }
            }
            None
        }
        rec(&mut self.root, id)
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

    /// Thermal case names in first-appearance order (M14). Parses give every temperature
    /// the default case "Thermal 1", so an unnamed thermal field is one case.
    pub fn thermal_cases(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for t in &self.temperatures {
            if !out.contains(&t.case) {
                out.push(t.case.clone());
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
                if let Some(p) = s.patch {
                    if p == 0 {
                        errs.push(format!(
                            "Region '{}' patch selector '{p}' is not a patch id (patches start at 1)",
                            r.id
                        ));
                    }
                    match self.find(&s.shape) {
                        None => {} // already reported as a missing shape
                        Some(n) => match &n.kind {
                            NodeKind::Mesh { patches, .. } => {
                                if p > *patches {
                                    errs.push(format!(
                                        "Region '{}' selects patch '{p}' of '{}' but the import segmented {patches} patches",
                                        r.id, s.shape
                                    ));
                                }
                            }
                            _ => {
                                errs.push(format!(
                                    "Region '{}' selects patch of '{}' which is not a mesh node",
                                    r.id, s.shape
                                ));
                            }
                        },
                    }
                }
                if s.face == Face::Side
                    && self
                        .find(&s.shape)
                        .is_some_and(|n| matches!(&n.kind, NodeKind::Mesh { .. }))
                {
                    errs.push(format!(
                        "Region '{}' faces 'side' of mesh '{}' — a mesh has no side face; use its patch selector or another face",
                        r.id, s.shape
                    ));
                }
                // An extend node exposes its swept surface's face (M7): the axis faces
                // label the skirt wall, but `all` and `side` are not faces of a derived
                // solid.
                if matches!(s.face, Face::All | Face::Side)
                    && s.patch.is_none()
                    && self
                        .find(&s.shape)
                        .is_some_and(|n| matches!(&n.kind, NodeKind::Extend { .. }))
                {
                    errs.push(format!(
                        "Region '{}' faces '{}' of extend '{}' — an extend exposes its swept face, not '{}'",
                        r.id,
                        s.face.name(),
                        s.shape,
                        s.face.name()
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
            if let LoadKind::Distributed { expr, dir, total } = &l.kind {
                let what = format!("load '{}'", l.id);
                match crate::loaddist::LoadFn::compile(expr) {
                    Ok(f) => errs.extend(f.static_checks(&what)),
                    Err(e) => errs.push(format!("{what}: fn: {e}")),
                }
                let (v, form) = match dir {
                    LoadDir::Global { v } => (v, "dir"),
                    LoadDir::Local { v } => (v, "local"),
                };
                if !v.iter().all(|c| c.is_finite()) {
                    errs.push(format!("{what}: {form} has a non-finite component"));
                } else if v.iter().all(|c| *c == 0.0) {
                    errs.push(format!("{what}: {form} is the zero vector"));
                }
                if expr.contains(['\n', '\r']) {
                    errs.push(format!(
                        "{what}: fn must be one line (separate bands with ';')"
                    ));
                }
                match total {
                    Some(t) if *t == 0.0 => errs.push(format!(
                        "{what}: total=0 makes the load vanish; give the intended resultant in N"
                    )),
                    Some(t) if !t.is_finite() => {
                        errs.push(format!("{what}: total= is not a finite number"))
                    }
                    _ => {}
                }
            }
        }
        // Temperatures (M14), placed like loads: missing regions name the same way. A case
        // name claimed by both namespaces cannot be solved honestly — the mechanical run
        // would ignore the temperature and the thermal run the loads — so name the clash.
        for t in &self.temperatures {
            if !region_ids.contains(&t.region.as_str()) {
                errs.push(format!(
                    "Temperature '{}' refers to missing region '{}'",
                    t.id, t.region
                ));
            }
            if !t.celsius.is_finite() {
                errs.push(format!(
                    "Temperature '{}' value is not a finite number",
                    t.id
                ));
            }
            for l in &self.loads {
                if l.case == t.case {
                    errs.push(format!(
                        "Case '{}' is claimed by load '{}' and temperature '{}' — mixed \
                         mechanical and thermal cases are not supported yet (give one of \
                         them another case name)",
                        t.case, l.id, t.id
                    ));
                }
            }
        }
        // Two temperatures on one region in one thermal case can only coexist when they
        // agree; differing values would fight over the same nodes. Conflicts the geometry
        // creates (two regions meeting at shared edge nodes) are named at solve time,
        // where the mesh is known.
        for (i, a) in self.temperatures.iter().enumerate() {
            for b in &self.temperatures[..i] {
                if a.case == b.case && a.region == b.region && a.celsius != b.celsius {
                    errs.push(format!(
                        "Temperatures '{}' and '{}' set conflicting values ({} °C and \
                         {} °C) on region '{}' in thermal case '{}' — one value per \
                         region per case",
                        a.id, b.id, a.celsius, b.celsius, a.region, a.case
                    ));
                }
            }
        }
        if let Some(t) = self.solve.t_ref {
            if !t.is_finite() {
                errs.push("solve tref= is not a finite number".into());
            }
        }
        // The thermal flow refuses a material with no sourced expansion coefficient
        // rather than solving with an assumed one.
        if !self.temperatures.is_empty() {
            if let Some(m) = crate::materials::find(self, &self.material) {
                if m.alpha.is_none() {
                    errs.push(format!(
                        "The thermal flow needs a thermal-expansion coefficient and material \
                         '{}' states none — pick a material whose sheet states a CTE, or \
                         give a custom material an a= value",
                        self.material
                    ));
                }
            }
        }
        if crate::materials::find(self, &self.material).is_none() {
            errs.push(format!("Unknown material '{}'", self.material));
        }
        // Per-node labels name a material the document knows; the missing-target case is
        // a parse error (`fsm::parse` resolves `material <id> on <shape>` at parse end),
        // so only unchecked ids remain. Mixed labels are a warning, not an error — see
        // `material_warnings`.
        Self::for_each_labelled(&self.root, &mut |n, m| {
            if crate::materials::find(self, m).is_none() {
                errs.push(format!("Unknown material '{}' on '{}'", m, n.id));
            }
        });
        // Colours travel to the 3MF writer as `displaycolor` (`#` + 6 or 8 hex digits);
        // a malformed value cannot render in the viewport either, so name it here.
        Self::for_each_coloured(&self.root, &mut |n, c| {
            if canonical_color(c).is_none() {
                errs.push(format!(
                    "Colour '{c}' on '{}' is not a hex colour ('#' plus 6 or 8 hex digits, e.g. '#21BB4C')",
                    n.id
                ));
            }
        });
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
        // Extends reference a region that must name at least one mesh patch or mesh face;
        // direction, length and the per-parent derived-grid budget are checked against the
        // regions and mesh nodes themselves.
        Self::check_extends(self, &mut errs);
        // Shell/offset modifiers on mesh operands bake a derived grid when the operand is a
        // single hydrated mesh (M7); the eval-time wrap they fall back to is only sound
        // within the ±3·voxel band of the clamped voxel field. Name the fallback conditions
        // so the user sees why a modifier is not exact before anything solves.
        Self::check_mesh_modifiers(&self.root, &mut errs);
        let mut meshes = 0usize;
        Self::check_meshes(&self.root, &mut meshes, &mut errs);
        // The preview binds a leaf's distance grid and (when it has patches) its patch grid
        // as sampler3D textures; WebGL2 guarantees 16 sampler units per fragment shader, so
        // eight leaf textures exhaust that guarantee. The solve is not limited by this
        // count, only the preview is — but a document that previews wrong is still refused
        // at validate time. Multi-object 3MF imports make this many leaves routinely (V12).
        if meshes > 8 {
            errs.push(format!(
                "{meshes} mesh nodes in this document; at most 8 are previewable (up to two \
                 sampler3D textures per node against WebGL2's 16-sampler fragment guarantee)"
            ));
        }
        errs
    }

    /// Recursively validate shell/offset modifiers against their group's operands (M7,
    /// step 2): a mesh operand bakes exactly only when the group holds a single
    /// non-suppressed child and that child is a hydrated mesh; the parameter must be usable
    /// and fit the derived-grid budget. Everything else keeps the eval-time wrap, whose
    /// distance values are only exact within ±3·voxel of the surface.
    fn check_mesh_modifiers(root: &[Node], errs: &mut Vec<String>) {
        fn rec(nodes: &[Node], errs: &mut Vec<String>) {
            for n in nodes {
                if let NodeKind::Group {
                    children,
                    modifiers,
                } = &n.kind
                {
                    let mesh_kids: Vec<&Node> = children
                        .iter()
                        .filter(|c| matches!(&c.kind, NodeKind::Mesh { .. }))
                        .collect();
                    let live = children.iter().filter(|c| !c.suppressed).count();
                    for m in modifiers {
                        match m {
                            Modifier::Shell { thickness, .. } => {
                                Document::check_mesh_modifier(
                                    &n.id, "shell", *thickness, &mesh_kids, live, errs,
                                );
                            }
                            Modifier::Offset { distance } => {
                                Document::check_mesh_modifier(
                                    &n.id, "offset", *distance, &mesh_kids, live, errs,
                                );
                            }
                            _ => {}
                        }
                    }
                    rec(children, errs);
                }
            }
        }
        rec(root, errs);
    }

    /// One shell/offset modifier against its group's mesh operands. `param` is the shell
    /// thickness or the offset distance; a mesh child bakes only when it is the group's
    /// single live child, its grid is hydrated, and the parameter is usable and within the
    /// derived-grid budget — anything else keeps the eval-time wrap, which is named here.
    fn check_mesh_modifier(
        group: &str,
        what: &str,
        param: f64,
        mesh_kids: &[&Node],
        live: usize,
        errs: &mut Vec<String>,
    ) {
        if mesh_kids.is_empty() {
            return; // a purely analytic group: the wrap is exact, nothing to name
        }
        if live > 1 {
            errs.push(format!(
                "Group '{group}' {what}s {live} shapes: a shell/offset over several operands keeps the eval-time form, whose distances are exact only within ±3·voxel of each surface"
            ));
            return;
        }
        let Some(mesh) = mesh_kids.first() else {
            return;
        };
        let NodeKind::Mesh {
            hash, voxel, bbox, ..
        } = &mesh.kind
        else {
            return;
        };
        if !param.is_finite() || param == 0.0 {
            errs.push(format!(
                "Group '{group}' {what} parameter {param} is not usable (zero or not finite)"
            ));
            return;
        }
        if !voxel.is_finite() || *voxel <= 0.0 || bbox.iter().any(|v| !v.is_finite()) {
            return; // the mesh's own field checks report these
        }
        // Mirror the field crate's budget estimate (same span math as the extend budget,
        // with the modifier's extent in place of the sweep length): a bake whose expanded
        // grid would exceed the cell budget is refused at compile and keeps the wrap.
        const MAX_DERIVED_CELLS: f64 = 6_000_000.0;
        const MAX_DERIVED_SPACING: f64 = 8.0;
        let extent = param.abs();
        let mut s = voxel.max(1e-9);
        let cells = loop {
            let margin = (extent / s).ceil() * s + 3.0 * s;
            let c: f64 = (0..3)
                .map(|a| ((bbox[a + 3] - bbox[a] + 2.0 * margin) / s).ceil() + 1.0)
                .fold(1.0, |a: f64, b: f64| a * b);
            if c <= MAX_DERIVED_CELLS || s >= MAX_DERIVED_SPACING {
                break c;
            }
            s = (s * 2.0).min(MAX_DERIVED_SPACING);
        };
        if cells > MAX_DERIVED_CELLS {
            errs.push(format!(
                "Group '{group}' {what} of {param} mm on mesh '{hash}' would need about {:.0} grid cells (budget {}); raise the mesh's voxel spacing or lower the {what}",
                cells,
                MAX_DERIVED_CELLS as u64
            ));
        }
    }

    /// Recursively validate extend nodes: each names a region with at least one mesh-patch
    /// selector, a finite non-zero direction, a positive length, and parent meshes whose
    /// derived sweep grid fits the cell budget.
    fn check_extends(doc: &Document, errs: &mut Vec<String>) {
        fn rec(doc: &Document, nodes: &[Node], errs: &mut Vec<String>) {
            for n in nodes {
                if let NodeKind::Extend { region, dir, len } = &n.kind {
                    match doc.regions.iter().find(|r| r.id == *region) {
                        None => {
                            errs.push(format!(
                                "Extend '{}' refers to missing region '{}'",
                                n.id, region
                            ));
                        }
                        Some(r) => {
                            let parents: std::collections::HashSet<&str> = r
                                .selectors
                                .iter()
                                .filter(|s| {
                                    if s.patch.is_some() {
                                        return true;
                                    }
                                    // Face-direction selectors sweep too — but only from a
                                    // mesh node (patch ids for primitives are meaningless and
                                    // the compile requires a hydrated grid to sweep from).
                                    s.patch.is_none()
                                        && s.face.axis().is_some()
                                        && doc.find(&s.shape).is_some_and(|n| {
                                            matches!(&n.kind, NodeKind::Mesh { .. })
                                        })
                                })
                                .map(|s| s.shape.as_str())
                                .collect();
                            if parents.is_empty() {
                                errs.push(format!(
                                    "Extend '{}' region '{}' names no mesh patch or face to sweep",
                                    n.id, region
                                ));
                            }
                            for pid in &parents {
                                if let Some(pn) = doc.find(pid) {
                                    if let NodeKind::Mesh { voxel, bbox, .. } = &pn.kind {
                                        Document::check_extend_budget(
                                            &n.id, pid, *voxel, bbox, dir, *len, errs,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    if dir.iter().all(|v| v.is_finite()) {
                        if dir.iter().all(|v| *v == 0.0) {
                            errs.push(format!("Extend '{}' has no direction (dir is zero)", n.id));
                        }
                    } else {
                        errs.push(format!("Extend '{}' direction is not finite", n.id));
                    }
                    if !len.is_finite() || *len <= 0.0 {
                        errs.push(format!("Extend '{}' length must be positive", n.id));
                    }
                }
                if let NodeKind::Group { children, .. } = &n.kind {
                    rec(doc, children, errs);
                }
            }
        }
        rec(doc, &doc.root, errs);
    }

    /// Upper-bound estimate of one parent's derived sweep grid. The builder spans the
    /// swept hull plus a three-cell skirt per side at the baked spacing; the hull lies
    /// inside the mesh's bounds and a node's grid spacing is never smaller than its
    /// voxel, so the same span at the voxel with no rounding is an upper bound. Trips
    /// before the preview waits on meshing work; the field crate's builder re-checks
    /// with the real spacing for paths that skip validate (same budget, mirrored here
    /// because the doc crate does not depend on the field crate).
    fn check_extend_budget(
        id: &str,
        parent: &str,
        voxel: f64,
        bbox: &[f64; 6],
        dir: &[f64; 3],
        len: f64,
        errs: &mut Vec<String>,
    ) {
        // A parent whose voxel or bounds are being reported separately is skipped here.
        if !voxel.is_finite() || voxel <= 0.0 || bbox.iter().any(|v| !v.is_finite()) {
            return;
        }
        let n = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        if !n.is_finite() || n <= 0.0 || !len.is_finite() || len <= 0.0 {
            return;
        }
        const MAX_SWEEP_CELLS: f64 = 6_000_000.0;
        let cells: f64 = (0..3)
            .map(|a| {
                let dn = dir[a] / n;
                (bbox[a + 3] - bbox[a] + dn.abs() * len + 6.0 * voxel) / voxel + 1.0
            })
            .fold(1.0, |a: f64, b: f64| a * b);
        if cells.is_finite() && cells > MAX_SWEEP_CELLS {
            errs.push(format!(
                "Extend '{}' sweeps mesh '{}' into about {:.0} voxel cells (budget {}, raise the mesh's voxel spacing or shorten the extension)",
                id,
                parent,
                cells,
                MAX_SWEEP_CELLS as u64
            ));
        }
    }

    /// Recursively validate mesh-node field values and count them.
    fn check_meshes(nodes: &[Node], meshes: &mut usize, errs: &mut Vec<String>) {
        for n in nodes {
            if let NodeKind::Mesh {
                hash,
                units,
                up,
                voxel,
                bbox,
                ..
            } = &n.kind
            {
                *meshes += 1;
                let hex_ok = hash.len() == 16
                    && hash
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
                if !hex_ok {
                    errs.push(format!(
                        "Mesh '{}' hash '{hash}' is not a 16-digit hex fingerprint",
                        n.id
                    ));
                }
                if !matches!(units.as_str(), "mm" | "cm" | "m" | "in") {
                    errs.push(format!(
                        "Mesh '{}' units '{units}' is not one of mm, cm, m, in",
                        n.id
                    ));
                }
                if !matches!(up.as_str(), "y" | "z") {
                    errs.push(format!("Mesh '{}' up-axis '{up}' is not y or z", n.id));
                }
                if !voxel.is_finite() || *voxel <= 0.0 {
                    errs.push(format!("Mesh '{}' voxel size must be positive", n.id));
                }
                if bbox.iter().any(|v| !v.is_finite()) {
                    errs.push(format!("Mesh '{}' bounds are not finite", n.id));
                } else if bbox[0] > bbox[3] || bbox[1] > bbox[4] || bbox[2] > bbox[5] {
                    errs.push(format!("Mesh '{}' bounds are inverted", n.id));
                }
            }
            if let NodeKind::Group { children, .. } = &n.kind {
                Self::check_meshes(children, meshes, errs);
            }
        }
    }

    /// A fresh unique id with the given prefix (`box1`, `box2`, …).
    pub fn fresh_id(&self, prefix: &str) -> String {
        let mut taken: std::collections::HashSet<String> = self.node_ids().into_iter().collect();
        taken.extend(self.regions.iter().map(|r| r.id.clone()));
        taken.extend(self.supports.iter().map(|r| r.id.clone()));
        taken.extend(self.loads.iter().map(|r| r.id.clone()));
        taken.extend(self.temperatures.iter().map(|r| r.id.clone()));
        (1..)
            .map(|i| format!("{prefix}{i}"))
            .find(|c| !taken.contains(c))
            .unwrap()
    }
}
