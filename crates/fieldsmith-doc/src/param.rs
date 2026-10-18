//! Numeric document parameters by address (GOAL v0.4 section D): a sweep value edits one
//! number of one named statement, `<id>.<attr>[.<index>]`, or one `let` by bare id
//! (`w`), which re-evaluates the lets and every expression that references them. Ids
//! resolve against the parsed document — bare ids are lets and never resolve to a
//! statement; dotted addresses run nodes, then loads, temperatures, convections, heat
//! fluxes — and a
//! number that a declared expression writes refuses a direct edit ("sweep the let that drives
//! it"). The addressable attributes are the model's own numeric fields, plus the
//! placement transform (`at`, `rotate`); everything else (region and support selectors,
//! extrude profile vertices, mesh and extend internals, solve settings) is outside the
//! grammar, which [`resolve`] states in its errors rather than leaving to the caller.

use crate::model::{Consumer, Document, Let, LoadKind, NodeKind};

/// One parsed address: statement id, attribute name, optional 0-based component index.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamAddr {
    pub id: String,
    pub attr: String,
    pub comp: Option<usize>,
}

impl ParamAddr {
    /// Parse a bare `let` name, `id.attr`, or `id.attr.n`. The id grammar is the
    /// node-id grammar (the parser enforces it on statements too); the index must be
    /// decimal.
    pub fn parse(addr: &str) -> Result<ParamAddr, String> {
        let bad = || {
            format!(
                "bad param address {addr:?}: expect a let name or id.attr / id.attr.n \
                 — e.g. beam.size.2, hole1.radius, pull.force.1, w"
            )
        };
        // A bare id is a let address; a dot starts the statement-attribute grammar.
        if !addr.contains('.') {
            return if is_ident(addr) {
                Ok(ParamAddr {
                    id: addr.into(),
                    attr: String::new(),
                    comp: None,
                })
            } else {
                Err(bad())
            };
        }
        let (id, rest) = addr.split_once('.').expect("checked for a dot");
        if id.is_empty() || rest.is_empty() {
            return Err(bad());
        }
        if !is_ident(id) {
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
        if self.attr.is_empty() {
            return self.id.clone();
        }
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

/// The id grammar shared with `.fsm` statement names.
fn is_ident(s: &str) -> bool {
    match s.chars().next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
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

/// Resolve the address against what the document actually names — bare ids against the
/// lets, dotted ones against nodes, then loads, then temperatures — and check the
/// attribute's component rule. No mutation.
pub fn resolve(doc: &Document, addr: &ParamAddr) -> Result<(), String> {
    let (id, attr) = (&addr.id, addr.attr.as_str());
    // A bare id is a let address and never a statement name: shapes are swept
    // through their attribute addresses.
    if attr.is_empty() {
        return if doc.lets.iter().any(|l| l.name == *id) {
            Ok(())
        } else {
            Err(format!("param {addr}: no parameter named \"{id}\""))
        };
    }
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
    if doc.convections.iter().any(|c| c.id == *id) {
        return match (attr, addr.comp) {
            ("h" | "tinf", None) => Ok(()),
            ("h" | "tinf", Some(_)) => {
                Err(format!("param {addr}: h and tinf take no component index"))
            }
            _ => Err(format!(
                "param {addr}: convection {id:?} has no numeric parameter {attr:?} — h, tinf"
            )),
        };
    }
    if doc.heat_fluxes.iter().any(|f| f.id == *id) {
        return match (attr, addr.comp) {
            ("q", None) => Ok(()),
            ("q", Some(_)) => Err(format!("param {addr}: q takes no component index")),
            _ => Err(format!(
                "param {addr}: heat flux {id:?} has no numeric parameter {attr:?} — q"
            )),
        };
    }
    Err(format!(
        "param {addr}: no node, load, temperature, convection or heat flux named {id:?}"
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
    if addr.attr.is_empty() {
        return apply_let(doc, &addr.id, value);
    }
    // A number a declared expression writes must be swept through its let: writing
    // over it here would leave the rest of the statement inconsistent with the
    // values the expression still references.
    if let Some(c) = doc
        .consumed
        .iter()
        .find(|c| c.target == addr.id && c.attr == addr.attr && c.comp == addr.comp)
    {
        return Err(format!(
            "param {addr}: written by an expression ('{}') — sweep the let that drives it",
            c.expr
        ));
    }
    write_plain(doc, &addr.id, &addr.attr, addr.comp, value);
    Ok(())
}

/// Re-evaluate every `let` in declaration order (each sees the values before it), then
/// every recorded consumer in parse order, and write the numbers back to the plain
/// fields the consumers address.
fn apply_let(doc: &mut Document, name: &str, value: f64) -> Result<(), String> {
    let slot = doc
        .lets
        .iter()
        .position(|l| l.name == name)
        .expect("resolve accepted this let");
    doc.lets[slot].value = value;
    let exprs: Vec<(String, String)> = doc
        .lets
        .iter()
        .map(|l| (l.name.clone(), l.expr.clone()))
        .collect();
    let mut values: Vec<(String, f64)> = Vec::new();
    for (oname, expr) in &exprs {
        // The applied let keeps the swept value; every other let re-evaluates in
        // declaration order, so derived lets chain. Re-evaluating the applied let
        // itself would clobber the sweep with its const-folded declaration value.
        let v = if *oname == name {
            value
        } else {
            let v = crate::fsm::eval_expr(expr, &values)
                .map_err(|m| format!("param {name}: the let '{oname}' does not evaluate: {m}"))?;
            if !v.is_finite() {
                return Err(format!(
                    "param {name}: the let '{oname}' value is not finite"
                ));
            }
            v
        };
        values.push((oname.clone(), v));
    }
    doc.lets = doc
        .lets
        .iter()
        .zip(&values)
        .map(|(l, (_, v))| Let {
            name: l.name.clone(),
            expr: l.expr.clone(),
            value: *v,
        })
        .collect();
    let consumed = doc.consumed.clone();
    for c in &consumed {
        let v = crate::fsm::eval_expr(&c.expr, &values).map_err(|m| {
            format!(
                "param {name}: the expression for '{}' does not evaluate: {m}",
                consumer_spec(c)
            )
        })?;
        if !v.is_finite() {
            return Err(format!(
                "param {name}: the expression for '{}' value is not finite",
                consumer_spec(c)
            ));
        }
        write_plain(doc, &c.target, &c.attr, c.comp, v);
    }
    Ok(())
}

/// The dotted spelling of one recorded consumer, for re-evaluation errors.
fn consumer_spec(c: &Consumer) -> String {
    match c.comp {
        Some(k) => format!("{}.{}.{}", c.target, c.attr, k),
        None => format!("{}.{}", c.target, c.attr),
    }
}

/// Write one number into the plain field of the addressed statement. Checked callers
/// only: `resolve` for addresses, parse/validate for recorded consumers.
fn write_plain(doc: &mut Document, target: &str, attr: &str, comp: Option<usize>, v: f64) {
    if let Some(node) = doc.find_mut(target) {
        match (&mut node.kind, attr, comp) {
            (NodeKind::Box { size, .. }, "size", Some(k)) => size[k] = v,
            (NodeKind::Box { round, .. }, "round", None) => *round = v,
            (NodeKind::Cylinder { radius, .. }, "radius", None) => *radius = v,
            (NodeKind::Cylinder { height, .. }, "height", None) => *height = v,
            (NodeKind::Cylinder { round, .. }, "round", None) => *round = v,
            (NodeKind::Sphere { radius }, "radius", None) => *radius = v,
            (NodeKind::Capsule { radius, .. }, "radius", None) => *radius = v,
            (NodeKind::Capsule { length, .. }, "length", None) => *length = v,
            (NodeKind::Torus { major, .. }, "major", None) => *major = v,
            (NodeKind::Torus { minor, .. }, "minor", None) => *minor = v,
            (NodeKind::Extrude { height, .. }, "height", None) => *height = v,
            (_, "at", Some(k)) => node.transform.translate[k] = v,
            (_, "rotate", Some(k)) => node.transform.rotate[k] = v,
            _ => unreachable!("the writer was checked against this statement"),
        }
        return;
    }
    if let Some(load) = doc.loads.iter_mut().find(|l| l.id == target) {
        match (&mut load.kind, attr, comp) {
            (LoadKind::Force { vector }, "force", Some(k)) => vector[k] = v,
            (LoadKind::Pressure { mpa }, "mpa", None) => *mpa = v,
            (LoadKind::Distributed { total, .. }, "total", None) => *total = Some(v),
            _ => unreachable!("the writer was checked against this statement"),
        }
        return;
    }
    if let Some(t) = doc.temperatures.iter_mut().find(|t| t.id == target) {
        t.celsius = v;
        return;
    }
    if let Some(c) = doc.convections.iter_mut().find(|c| c.id == target) {
        match (attr, comp) {
            ("h", None) => c.h = v,
            ("tinf", None) => c.t_inf = v,
            _ => unreachable!("the writer was checked against this statement"),
        }
        return;
    }
    if let Some(f) = doc.heat_fluxes.iter_mut().find(|f| f.id == target) {
        f.q = v;
        return;
    }
    unreachable!("the writer was checked against this document");
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
        // A bare id is a let address now, so the bad spellings keep their dots.
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

    const LET_DOC: &str = "fieldsmith 1
name \"params\"
custommaterial steel200 name=\"Steel\" e=200000 nu=0.3 yield=250 density=7.85
material steel200
let w = 4
let bolt = 2 * w + 2
box beam size=(100, 2 * w, 2 * w) at=(50, w, w)
cylinder pin r=w h=bolt at=(20, 5, 5)
region root = beam.nx
support clamp root fixed=xyz
load push root force=(0, 0, -(w * 25))
temperature hot root temp=80 + w";

    fn let_doc() -> Document {
        fsm::parse(LET_DOC).expect("parse")
    }

    #[test]
    fn bare_addresses_resolve_lets() {
        // A bare id addresses a let now; the dotted spellings stay the node grammar.
        assert!(ParamAddr::parse("w").is_ok());
        assert!(ParamAddr::parse("beam.").is_err());
        let d = let_doc();
        resolve(&d, &ParamAddr::parse("w").unwrap()).expect("let resolves");
        let e = resolve(&d, &ParamAddr::parse("notw").unwrap()).unwrap_err();
        assert!(e.contains("no parameter named \"notw\""), "{e}");
        // An id that names a node cannot be addressed bare.
        let e2 = resolve(&d, &ParamAddr::parse("beam").unwrap()).unwrap_err();
        assert!(e2.contains("no parameter named \"beam\""), "{e2}");
    }

    #[test]
    fn applying_to_a_let_recomputes_its_consumers() {
        let mut d = let_doc();
        let a = ParamAddr::parse("w").unwrap();
        apply(&mut d, &a, 5.0).expect("apply");
        let beam = d.find("beam").unwrap();
        let NodeKind::Box { size, .. } = &beam.kind else {
            panic!("box")
        };
        assert_eq!(*size, [100.0, 10.0, 10.0]);
        assert_eq!(beam.transform.translate, [50.0, 5.0, 5.0]);
        assert_eq!(
            d.temperatures
                .iter()
                .find(|t| t.id == "hot")
                .unwrap()
                .celsius,
            85.0
        );
        // The derived let revaluated with the new value.
        assert_eq!(d.lets.iter().find(|l| l.name == "bolt").unwrap().value, 12.0);
        assert!(apply(&mut d, &ParamAddr::parse("notw").unwrap(), 1.0).is_err());
        assert!(apply(&mut d, &a, f64::NAN).is_err());
    }

    #[test]
    fn driven_attrs_refuse_direct_writes() {
        let mut d = let_doc();
        let e = apply(&mut d, &ParamAddr::parse("beam.size.2").unwrap(), 9.0).unwrap_err();
        assert!(e.contains("written by an expression"), "{e}");
        assert!(e.contains("sweep the let that drives it"), "{e}");
        // An undriven component of the same statement still writes.
        apply(&mut d, &ParamAddr::parse("beam.size.0").unwrap(), 120.0).expect("apply");
        let beam = d.find("beam").unwrap();
        let NodeKind::Box { size, .. } = &beam.kind else {
            panic!("box")
        };
        assert_eq!(size[0], 120.0);
    }

    const CONV_DOC: &str = "\
fieldsmith 1
name \"params\"
custommaterial steel200 name=\"Steel\" e=200000 nu=0.3 yield=250 density=7.85
material steel200
let hw = 0.01
box beam size=(100, 10, 10) at=(50, 5, 5)
region hotface = beam.px
region coldface = beam.nx
temperature fixer hotface temp=80
convection air hotface h=2 * hw tinf=25
let qv = 1e-3
heatflux lead coldface q=10 * qv
";

    fn conv_doc() -> Document {
        fsm::parse(CONV_DOC).expect("parse")
    }

    #[test]
    fn resolves_applies_and_refuses_convection_params() {
        let d = conv_doc();
        for addr in ["air.h", "air.tinf"] {
            resolve(&d, &ParamAddr::parse(addr).unwrap()).unwrap_or_else(|e| panic!("{addr}: {e}"));
        }
        // A component index is refused; so is an attribute the band does not carry.
        let e = resolve(&d, &ParamAddr::parse("air.h.0").unwrap()).unwrap_err();
        assert!(e.contains("take no component index"), "{e}");
        let e = resolve(&d, &ParamAddr::parse("air.pressure").unwrap()).unwrap_err();
        assert!(e.contains("no numeric parameter \"pressure\" — h, tinf"), "{e}");
        let e = resolve(&d, &ParamAddr::parse("ghost.h").unwrap()).unwrap_err();
        assert!(
            e.contains(
                "no node, load, temperature, convection or heat flux named \"ghost\"",
            ),
            "{e}"
        );
        // Plain edits land on the fields.
        let mut d = conv_doc();
        apply(&mut d, &ParamAddr::parse("air.tinf").unwrap(), 60.0).expect("apply");
        assert_eq!(d.convections[0].t_inf, 60.0);
        // A driven h refuses a direct write; sweeping its let recomputes it.
        let e = apply(&mut d, &ParamAddr::parse("air.h").unwrap(), 0.1).unwrap_err();
        assert!(e.contains("written by an expression"), "{e}");
        apply(&mut d, &ParamAddr::parse("hw").unwrap(), 0.02).expect("sweep the let");
        assert_eq!(d.convections[0].h, 0.04);
    }

    #[test]
    fn resolves_applies_and_refuses_heatflux_params() {
        let d = conv_doc();
        resolve(&d, &ParamAddr::parse("lead.q").unwrap()).unwrap_or_else(|e| panic!("q: {e}"));
        // A component index is refused; so is an attribute the statement does not carry.
        let e = resolve(&d, &ParamAddr::parse("lead.q.0").unwrap()).unwrap_err();
        assert!(e.contains("q takes no component index"), "{e}");
        let e = resolve(&d, &ParamAddr::parse("lead.h").unwrap()).unwrap_err();
        assert!(e.contains("no numeric parameter \"h\" — q"), "{e}");
        // A driven q refuses a direct write; sweeping its let recomputes it.
        let mut d = conv_doc();
        let e = apply(&mut d, &ParamAddr::parse("lead.q").unwrap(), 0.5).unwrap_err();
        assert!(e.contains("written by an expression"), "{e}");
        apply(&mut d, &ParamAddr::parse("qv").unwrap(), 2e-3).expect("sweep the let");
        assert_eq!(d.heat_fluxes[0].q, 0.02);
    }
}
