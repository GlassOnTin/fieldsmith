//! Built-in material library.
//!
//! Values are for *printed* parts where the manufacturer publishes them, because that is what
//! the user will make. Printed parts are anisotropic and process-dependent; these are
//! reference values for comparison, not guaranteed design allowables. The UI shows the source.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub id: String,
    pub name: String,
    /// Young's modulus (MPa).
    pub e: f64,
    /// Poisson's ratio.
    pub nu: f64,
    /// Yield (or 0.2 % proof) strength (MPa).
    pub yield_strength: f64,
    /// Density (g/cm³).
    pub density: f64,
    /// Default thinnest printable wall for the usual process (mm).
    pub min_wall: f64,
    /// Where the numbers come from.
    pub source: String,
}

#[allow(clippy::too_many_arguments)]
fn m(
    id: &str,
    name: &str,
    e: f64,
    nu: f64,
    ys: f64,
    rho: f64,
    wall: f64,
    source: &str,
) -> Material {
    Material {
        id: id.into(),
        name: name.into(),
        e,
        nu,
        yield_strength: ys,
        density: rho,
        min_wall: wall,
        source: source.into(),
    }
}

/// The built-in library, in display order.
pub fn library() -> Vec<Material> {
    vec![
        m("pla", "PLA (FDM)", 2300.0, 0.36, 51.0, 1.24, 0.8,
          "Prusament PLA TDS: printed ISO 527-1 horizontal specimens, E 2.3 GPa, yield 51 MPa; density 1.24 g/cm³. ν typical literature value."),
        m("petg", "PETG (FDM)", 1500.0, 0.38, 47.0, 1.27, 0.8,
          "Prusament PETG TDS: printed ISO 527-1 horizontal specimens, E 1.5 GPa, yield 47 MPa; density 1.27 g/cm³ (ISO 1183). ν typical literature value."),
        m("abs", "ABS (FDM)", 2200.0, 0.35, 40.0, 1.04, 0.8,
          "Typical printed ABS (Prusament ABS class): E 2.2–2.4 GPa, yield 40–50 MPa; lower bounds used. Density 1.04 g/cm³."),
        m("pa12", "PA12 (SLS)", 1650.0, 0.40, 45.0, 0.93, 0.8,
          "EOS PA 2200 material data sheet: tensile modulus 1650 MPa, tensile strength ≈ 45 MPa, part density ≈ 0.93 g/cm³. ν typical literature value."),
        m("alsi10mg", "AlSi10Mg (LPBF)", 70000.0, 0.33, 230.0, 2.67, 0.4,
          "EOS AlSi10Mg data sheet class values: Rp0.2 ≈ 230–260 MPa (lower bound used), E ≈ 70 GPa, density 2.67 g/cm³."),
        m("ti64", "Ti-6Al-4V (LPBF)", 110000.0, 0.34, 960.0, 4.41, 0.4,
          "EOS Ti64 data sheet: E 110 ± 7 GPa, Rp0.2 1030 ± 70 MPa (mean − 1σ used), density 4.41 g/cm³."),
        m("ss316l", "316L stainless (LPBF)", 180000.0, 0.30, 470.0, 7.98, 0.4,
          "EOS StainlessSteel 316L data sheet: E ≈ 180–185 GPa, Rp0.2 ≈ 470–530 MPa (lower bound used), density 7.98 g/cm³."),
    ]
}

/// Look up a material by id: custom materials in the document first, then the library.
pub fn find(doc: &crate::Document, id: &str) -> Option<Material> {
    doc.custom_materials
        .iter()
        .find(|m| m.id == id)
        .cloned()
        .or_else(|| library().into_iter().find(|m| m.id == id))
}
