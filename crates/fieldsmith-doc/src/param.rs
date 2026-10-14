//! Numeric document parameters by address (GOAL v0.4 section D): a sweep value edits one
//! number of one named statement, `<id>.<attr>[.<index>]`. Ids resolve against the parsed
//! document — nodes first, then loads, then temperatures — and applying a value is an edit
//! of that one number. The addressable attributes are the model's own numeric fields,
//! plus the placement transform (`at`, `rotate`); everything else (region and support
//! selectors, extrude profile vertices, mesh and extend internals, solve settings) is
//! outside the grammar, which [`resolve`] states in its errors rather than leaving to
//! the caller.

use crate::model::{Document, LoadKind, NodeKind};

/// One parsed address: statement id, attribute name, optional 0-based component index.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamAddr {
    pub id: String,
    pub attr: String,
    pub comp: Option<usize>,
}

impl ParamAddr {
    /// Parse `id.attr` or `id.attr.n`. The id grammar is the node-id grammar (the
    /// parser enforces it on statements too); the index must be decimal.
    pub fn parse(addr: &str) -> Result<ParamAddr, String> {
        let bad = || {
            format!(
                "bad param address {addr:?}: expect id.attr or id.attr.n — e.g. \
                 beam.size.2, hole1.radius, pull.force.1"
            )
        };
        let (id, rest) = addr.split_once('.').ok_or_else(bad)?;
        if id.is_empty() || rest.is_empty() {
            return Err(bad());
        }
        let first = id.chars().next().expect("checked non-empty");
        if !(first.is_ascii_alphabetic() || first == '_')
            || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(bad());
        }
        let parts: Vec<&str> = rest.split('.').collect();
        let (attr, comp) = match parts.as_slice() {
            [attr] => (*attr, None),
            [attr, n] => {
                let comp: usize = n.parse().map_err(|_| bad())?;
                (*attr, Some(comp))
            }
            _ => return Err(bad()),
        };
        if attr.is_empty() {
            return Err(bad());
        }
        Ok(ParamAddr {
            id: id.to_string(),
            attr: attr.to_string(),
            comp,
        })
    }

    /// The full address as written, for error texts and the envelope.
    pub fn text(&self) -> String {
        match self.comp {
            Some(n) => format!("{}.{}.{}", self.id, self.attr, n),
            None => format!("{}.{}", self.id, self.attr),
        }
    }

    /// The address without its component index, for the "needs a component index"
    /// error that shows the valid spellings.
    fn text_without_comp(&self) -> String {
        format!("{}.{}", self.id, self.attr)
    }
}

impl std::fmt::Display for ParamAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text())
    }
}

/// `attr.k` requires the index; 0..2 for the 3-component vectors this grammar covers.
fn need_index(addr: &ParamAddr, attr: &str) -> Result<usize, String> {
    addr.comp.ok_or_else(|| {
        format!(
            "param {addr}: {attr} needs a component index — {}.0 .. {}.2",
            addr.text_without_comp(),
            addr.text_without_comp()
        )
    })
}

fn check_index(addr: &ParamAddr, attr: &str, k: usize) -> Result<(), String> {
    match k {
        0..=2 => Ok(()),
        _ => Err(format!(
            "param {addr}: {}.{} is out of range ({}.0 .. {}.2)",
            attr,
            k,
            addr.text_without_comp(),
            addr.text_without_comp()
        )),
    }
}

/// The scalar parameters take no component index.
fn no_index(addr: &ParamAddr, attr: &str) -> Result<(), String> {
    match addr.comp {
        None => Ok(()),
        Some(_) => Err(format!("param {addr}: {attr} takes no component index")),
    }
}

/// Resolve the address against what the document actually names — nodes first, then
/// loads, then temperatures — and check the attribute's component rule. No mutation.
pub fn resolve(doc: &Document, addr: &ParamAddr) -> Result<(), String> {
    let (id, attr) = (&addr.id, addr.attr.as_str());
    if let Some(node) = doc.find(id) {
        return match (&node.kind, attr) {
            // `at` and `rotate` address any node's placement, meshes and groups included.
            (_, "at" | "rotate") => {
                let k = need_index(addr, attr)?;
                check_index(addr, attr, k)
            }
            (NodeKind::Box { .. }, "size") => {
                let k = need_index(addr, attr)?;
                check_index(addr, attr, k)
            }
            (NodeKind::Box { .. }, "round")
            | (NodeKind::Cylinder { .. }, "radius" | "height" | "round")
            | (NodeKind::Sphere { .. }, "radius")
            | (NodeKind::Capsule { .. }, "radius" | "length")
            | (NodeKind::Torus { .. }, "major" | "minor")
            | (NodeKind::Extrude { .. }, "height") => no_index(addr, attr),
            (NodeKind::Group { .. }, _)
            | (NodeKind::Mesh { .. }, _)
            | (NodeKind::Extend { .. }, _) => Err(format!(
                "param {addr}: node {id:?} ({}) has no numeric parameter {attr:?} — {}",
                kind_name(&node.kind),
                attrs_note(&node.kind)
            )),
            _ => Err(format!(
                "param {addr}: node {id:?} ({}) has no numeric parameter {attr:?} — {}",
                kind_name(&node.kind),
                attrs_note(&node.kind)
            )),
        };
    }
    if let Some(load) = doc.loads.iter().find(|l| l.id == *id) {
        return match (&load.kind, attr) {
            (LoadKind::Force { .. }, "force") => {
                let k = need_index(addr, attr)?;
                check_index(addr, attr, k)
            }
            (LoadKind::Pressure { .. }, "mpa") | (LoadKind::Distributed { .. }, "total") => {
                no_index(addr, attr)
            }
            _ => {
                let takes = match &load.kind {
                    LoadKind::Force { .. } => "force[0-2] (force.n)",
                    LoadKind::Pressure { .. } => "mpa",
                    LoadKind::Distributed { .. } => "total",
                };
                Err(format!(
                    "param {addr}: load {id:?} ({}) has no numeric parameter \
                     {attr:?} — its parameters are {takes}",
                    load_kind_name(&load.kind)
                ))
            }
        };
    }
    if doc.temperatures.iter().any(|t| t.id == *id) {
        return match (attr, addr.comp) {
            ("celsius", None) => Ok(()),
            ("celsius", Some(_)) => Err(format!("param {addr}: celsius takes no component index")),
            _ => Err(format!(
                "param {addr}: temperature {id:?} has no numeric parameter {attr:?} — celsius"
            )),
        };
    }
    Err(format!(
        "param {addr}: no node, load or temperature named {id:?}"
    ))
}

/// Apply one numeric value to the addressed parameter. The address must have resolved
/// against this document (call [`resolve`] first at request time); [`apply`] re-runs
/// the same checks so a stale caller cannot write outside the grammar.
pub fn apply(doc: &mut Document, addr: &ParamAddr, value: f64) -> Result<(), String> {
    resolve(doc, addr)?;
    if !value.is_finite() {
        return Err(format!("param {addr}: value {value} is not finite"));
    }
    if let Some(node) = doc.find_mut(&addr.id) {
        // resolve() proved every access below is in-bounds.
        match (&mut node.kind, addr.attr.as_str(), addr.comp) {
            (NodeKind::Box { size, .. }, "size", Some(k)) => size[k] = value,
            (NodeKind::Box { round, .. }, "round", None) => {
                *round = value;
            }
            (NodeKind::Cylinder { radius, .. }, "radius", None) => {
                *radius = value;
            }
            (NodeKind::Cylinder { height, .. }, "height", None) => {
                *height = value;
            }
            (NodeKind::Cylinder { round, .. }, "round", None) => {
                *round = value;
            }
            (NodeKind::Sphere { radius }, "radius", None) => {
                *radius = value;
            }
            (NodeKind::Capsule { radius, .. }, "radius", None) => {
                *radius = value;
            }
            (NodeKind::Capsule { length, .. }, "length", None) => {
                *length = value;
            }
            (NodeKind::Torus { major, .. }, "major", None) => {
                *major = value;
            }
            (NodeKind::Torus { minor, .. }, "minor", None) => {
                *minor = value;
            }
            (NodeKind::Extrude { height, .. }, "height", None) => {
                *height = value;
            }
            (_, "at", Some(k)) => node.transform.translate[k] = value,
            (_, "rotate", Some(k)) => node.transform.rotate[k] = value,
            _ => unreachable!("resolve accepted this attr"),
        }
        return Ok(());
    }
    if let Some(load) = doc.loads.iter_mut().find(|l| l.id == addr.id) {
        match (&mut load.kind, addr.attr.as_str(), addr.comp) {
            (LoadKind::Force { vector }, "force", Some(k)) => vector[k] = value,
            (LoadKind::Pressure { mpa }, "mpa", None) => {
                *mpa = value;
            }
            (LoadKind::Distributed { total, .. }, "total", None) => *total = Some(value),
            _ => unreachable!("resolve accepted this attr"),
        }
        return Ok(());
    }
    if let Some(t) = doc.temperatures.iter_mut().find(|t| t.id == addr.id) {
        t.celsius = value;
        return Ok(());
    }
    unreachable!("resolve accepted this id")
}

/// The numeric attribute names one statement kind accepts, for the error that names
/// what a failed address could have been.
fn attrs_note(kind: &NodeKind) -> String {
    let shape = match kind {
        NodeKind::Box { .. } => "size[0-2] (size.n), round",
        NodeKind::Cylinder { .. } => "radius, height, round",
        NodeKind::Sphere { .. } => "radius",
        NodeKind::Capsule { .. } => "radius, length",
        NodeKind::Torus { .. } => "major, minor",
        NodeKind::Extrude { .. } => "height (profile vertices are not sweep parameters)",
        NodeKind::Group { .. } => "a group carries no numbers; its shapes do",
        NodeKind::Mesh { .. } => {
            "a mesh import carries no sweep parameters (its grid was \
                                  baked at import)"
        }
        NodeKind::Extend { .. } => "an extend's dir/len are fixed at declaration",
    };
    format!("{shape} — and at[0-2], rotate[0-2] for the placement of any node")
}

fn kind_name(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Box { .. } => "box",
        NodeKind::Cylinder { .. } => "cylinder",
        NodeKind::Sphere { .. } => "sphere",
        NodeKind::Capsule { .. } => "capsule",
        NodeKind::Torus { .. } => "torus",
        NodeKind::Extrude { .. } => "extrude",
        NodeKind::Group { .. } => "group",
        NodeKind::Mesh { .. } => "mesh",
        NodeKind::Extend { .. } => "extend",
    }
}

fn load_kind_name(kind: &LoadKind) -> &'static str {
    match kind {
        LoadKind::Force { .. } => "force",
        LoadKind::Pressure { .. } => "pressure",
        LoadKind::Distributed { .. } => "distributed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsm;

    const DOC: &str = "\
fieldsmith 1
name \"params\"
custommaterial steel200 name=\"Steel\" e=200000 nu=0.3 yield=250 density=7.85
material steel200
box beam size=(100, 10, 10) at=(50, 5, 5)
cylinder pin r=4 h=10 at=(20, 5, 5) rotate=(90, 0, 0)
region root = beam.nx
support clamp root fixed=xyz
load push root force=(0, 0, -100)
temperature hot root temp=80";

    fn doc() -> Document {
        fsm::parse(DOC).expect("parse")
    }

    #[test]
    fn parses_component_and_plain_addresses() {
        let a = ParamAddr::parse("beam.size.2").expect("parse");
        assert_eq!(
            a,
            ParamAddr {
                id: "beam".into(),
                attr: "size".into(),
                comp: Some(2)
            }
        );
        let b = ParamAddr::parse("pin.radius").expect("parse");
        assert_eq!(b.comp, None);
        // Bad shapes are the named error, not a panic on the index or the id grammar.
        assert!(ParamAddr::parse("beam").is_err());
        assert!(ParamAddr::parse("beam.").is_err());
        assert!(ParamAddr::parse(".size").is_err());
        assert!(ParamAddr::parse("beam.size.1.2").is_err());
        assert!(ParamAddr::parse("beam.size.x").is_err());
        assert!(ParamAddr::parse("bea m.size").is_err());
    }

    #[test]
    fn resolves_shape_placement_load_and_temperature() {
        let d = doc();
        // Shapes and placements resolve.
        for addr in [
            "beam.size.2",
            "beam.round",
            "pin.radius",
            "pin.at.0",
            "beam.rotate.1",
            "hot.celsius",
        ] {
            resolve(&d, &ParamAddr::parse(addr).unwrap()).unwrap_or_else(|e| panic!("{addr}: {e}"));
        }
        // round needs no index; a number on it is refused; size needs one.
        assert!(resolve(&d, &ParamAddr::parse("beam.round.1").unwrap()).is_err());
        assert!(resolve(&d, &ParamAddr::parse("beam.size").unwrap()).is_err());
        assert!(resolve(&d, &ParamAddr::parse("beam.size.3").unwrap()).is_err());
        assert!(resolve(&d, &ParamAddr::parse("hot.celsius.0").unwrap()).is_err());
        // A cylinder has no major; unknown attrs and unknown ids are named errors.
        assert!(resolve(&d, &ParamAddr::parse("pin.major").unwrap()).is_err());
        assert!(resolve(&d, &ParamAddr::parse("beam.nosuch").unwrap()).is_err());
        assert!(resolve(&d, &ParamAddr::parse("nothere.size.0").unwrap()).is_err());
        // The force load takes force.n, not a scalar.
        assert!(resolve(&d, &ParamAddr::parse("push.total").unwrap()).is_err());
        assert!(resolve(&d, &ParamAddr::parse("push.force").unwrap()).is_err());
        assert!(resolve(&d, &ParamAddr::parse("push.force.2").unwrap()).is_ok());
    }

    #[test]
    fn applies_edits_and_reports_non_finite() {
        let mut d = doc();
        let a = ParamAddr::parse("beam.size.2").unwrap();
        apply(&mut d, &a, 12.5).expect("apply");
        let beam = d.find("beam").unwrap();
        let NodeKind::Box { size, .. } = &beam.kind else {
            panic!("box")
        };
        assert_eq!(size[2], 12.5);
        // The load and the temperature round-trip the same way.
        let f = ParamAddr::parse("push.force.2").unwrap();
        apply(&mut d, &f, -40.0).expect("apply");
        let push = d.loads.iter().find(|l| l.id == "push").unwrap();
        assert_eq!(
            match &push.kind {
                LoadKind::Force { vector } => vector[2],
                _ => panic!("force"),
            },
            -40.0
        );
        let t = ParamAddr::parse("hot.celsius").unwrap();
        apply(&mut d, &t, 120.0).expect("apply");
        assert_eq!(
            d.temperatures
                .iter()
                .find(|t| t.id == "hot")
                .unwrap()
                .celsius,
            120.0
        );
        // A rotate component edits the transform.
        let r = ParamAddr::parse("pin.rotate.1").unwrap();
        apply(&mut d, &r, 45.0).expect("apply");
        assert_eq!(d.find("pin").unwrap().transform.rotate[1], 45.0);
        assert!(apply(&mut d, &a, f64::NAN).is_err());
        assert!(apply(&mut d, &a, f64::INFINITY).is_err());
    }
}
