//! Built-in material library.
//!
//! Values are for *printed* parts where the manufacturer publishes them, because that is what
//! the user will make. Printed parts are anisotropic and process-dependent; these are
//! reference values for comparison, not guaranteed design allowables. The UI shows the source.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MaterialRole {
    /// Load-bearing solid continuum volume assembled into 3D structural FEA.
    #[default]
    Structural,
    /// Non-structural fluid, melt, or gas volume (excluded from 3D solid Hookean FEA stress assembly).
    Fluid,
}

impl MaterialRole {
    pub fn is_structural(&self) -> bool {
        matches!(self, Self::Structural)
    }
}

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
    /// Linear thermal expansion coefficient (1/°C). `None` when no usable value is
    /// sourced — a thermal document on this material is then refused (see
    /// `Document::validate`), not silently solved with a made-up one.
    #[serde(default)]
    pub alpha: Option<f64>,
    /// Thermal conductivity `k` in W/(m·K).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub k: Option<f64>,
    /// Inter-layer (build-axis) tensile bond strength `S_z` (MPa). When `None`,
    /// [`Material::effective_layer_strengths`] applies the documented default knockdown
    /// (`0.60 × yield_strength`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yield_z: Option<f64>,
    /// Inter-layer (build-axis) shear bond strength `S_s` (MPa). When `None`,
    /// [`Material::effective_layer_strengths`] applies the von Mises shear yield default
    /// (`yield_strength / √3`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shear_z: Option<f64>,
    /// Hollomon isotropic J2 strain-hardening exponent `n ∈ (0, 1]` (`σ_flow = σ_y (ε/ε_y)^n`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardening_n: Option<f64>,
    /// Hollomon strength coefficient `K` (MPa) in `σ = K ε^n`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardening_k: Option<f64>,
    /// 2-parameter Weibull brittle flaw modulus `m > 0` (`P_f = 1 − exp(−∫(σ₁/σ₀)^m dV/V₀)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weibull_m: Option<f64>,
    /// Continuum phase role in 3D FEA (`structural` by default; `fluid` excludes liquid/gas volumes from Hookean solid stress).
    #[serde(default, skip_serializing_if = "MaterialRole::is_structural")]
    pub role: MaterialRole,
    /// Where the numbers come from.
    pub source: String,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            id: "custom".into(),
            name: "Custom".into(),
            e: 2000.0,
            nu: 0.35,
            yield_strength: 45.0,
            density: 1.25,
            min_wall: 0.8,
            alpha: None,
            k: None,
            yield_z: None,
            shear_z: None,
            hardening_n: None,
            hardening_k: None,
            weibull_m: None,
            role: MaterialRole::Structural,
            source: "User supplied".into(),
        }
    }
}

/// Default inter-layer tensile knockdown factor relative to bulk `yield_strength`
/// when `yield_z` is omitted on a material.
pub const DEFAULT_YIELD_Z_RATIO: f64 = 0.60;

impl Material {
    /// Effective inter-layer tensile and shear bond strengths `(S_z, S_s)` in MPa:
    /// uses explicit `yield_z` / `shear_z` when declared, falling back to
    /// `0.60 × yield_strength` and `yield_strength / √3` otherwise.
    pub fn effective_layer_strengths(&self) -> (f64, f64) {
        let sz = self
            .yield_z
            .unwrap_or(DEFAULT_YIELD_Z_RATIO * self.yield_strength);
        let ss = self
            .shear_z
            .unwrap_or(self.yield_strength / 3.0_f64.sqrt());
        (sz, ss)
    }
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
    alpha: Option<f64>,
    k: Option<f64>,
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
        alpha,
        k,
        yield_z: None,
        shear_z: None,
        hardening_n: None,
        hardening_k: None,
        weibull_m: None,
        role: MaterialRole::Structural,
        source: source.into(),
    }
}

#[allow(clippy::too_many_arguments)]
fn m_ortho(
    id: &str,
    name: &str,
    e: f64,
    nu: f64,
    ys: f64,
    rho: f64,
    wall: f64,
    alpha: Option<f64>,
    k: Option<f64>,
    yield_z: Option<f64>,
    shear_z: Option<f64>,
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
        alpha,
        k,
        yield_z,
        shear_z,
        hardening_n: None,
        hardening_k: None,
        weibull_m: None,
        role: MaterialRole::Structural,
        source: source.into(),
    }
}

/// The built-in library, in display order.
pub fn library() -> Vec<Material> {
    let mut sic = m("sic_jwst", "SiC Space-Optics Ceramic (Weibull Brittle)", 410000.0, 0.17, 340.0, 3.10, 1.0, Some(2.2e-6), Some(120.0),
          "Sintered Silicon Carbide (Boostec SiC space-optics bench class): E = 410 GPa, ν = 0.17, characteristic flexural/tensile Weibull scale σ₀ = 340 MPa (Weibull modulus m = 10.5), ρ = 3.10 g/cm³, CTE = 2.2 ×10⁻⁶/°C.");
    sic.weibull_m = Some(10.5);

    let mut dc04 = m("mat_dc04", "DC04 Deep-Drawing Mild Steel (EN 10130)", 210000.0, 0.30, 180.0, 7.85, 0.4, Some(12.0e-6), Some(50.0),
          "EN 10130 DC04 (1.0338) cold-rolled low-carbon forming sheet steel: E = 210 GPa, ν = 0.30, Rp0.2 = 180 MPa, Hollomon n = 0.22, K = 530 MPa, ρ = 7.85 g/cm³, CTE = 12.0 ×10⁻⁶/°C, k = 50 W/(m·K).");
    dc04.hardening_n = Some(0.22);
    dc04.hardening_k = Some(530.0);

    let mut al5754 = m("al5754_clinch", "EN AW-5754-H111 Clinching Aluminium Sheet", 70000.0, 0.33, 65.0, 2.67, 0.4, Some(23.0e-6), Some(150.0),
          "EN AW-5754-H111 (AlMg3) annealed sheet metal: E = 70 GPa, ν = 0.33, σ_y = 65 MPa, Hollomon n = 0.20, K = 390 MPa, ρ = 2.67 g/cm³, CTE = 23.0 ×10⁻⁶/°C, k = 150 W/(m·K).");
    al5754.hardening_n = Some(0.20);
    al5754.hardening_k = Some(390.0);

    vec![
        m("pla", "PLA (FDM)", 2300.0, 0.36, 51.0, 1.24, 0.8, Some(96e-6), Some(0.18),
          "Prusament PLA TDS: printed ISO 527-1 horizontal specimens, E 2.3 GPa, yield 51 MPa; density 1.24 g/cm³. ν typical literature value. CTE 96 ×10⁻⁶/°C, printed-part literature value (FBG-measured FDM PLA, 25–50 °C); rises steeply near Tg."),
        m("petg", "PETG (FDM)", 1500.0, 0.38, 47.0, 1.27, 0.8, Some(70e-6), Some(0.24),
          "Prusament PETG TDS: printed ISO 527-1 horizontal specimens, E 1.5 GPa, yield 47 MPa; density 1.27 g/cm³ (ISO 1183). ν typical literature value. CTE 70 ×10⁻⁶/°C typical literature value (printed PETG class ≈ 68–80 µm/m·K, 25–50 °C)."),
        m("abs", "ABS (FDM)", 2200.0, 0.35, 40.0, 1.04, 0.8, Some(86e-6), Some(0.22),
          "Typical printed ABS (Prusament ABS class): E 2.2–2.4 GPa, yield 40–50 MPa; lower bounds used. Density 1.04 g/cm³. CTE 86 ×10⁻⁶/°C, printed-part literature value (FBG-measured FDM ABS, 25–50 °C)."),
        m("pa12", "PA12 (SLS)", 1650.0, 0.40, 45.0, 0.93, 0.8, Some(109e-6), Some(0.28),
          "EOS PA 2200 material data sheet: tensile modulus 1650 MPa, tensile strength ≈ 45 MPa, part density ≈ 0.93 g/cm³. ν typical literature value. CTE 1.09 ×10⁻⁴/°C (ISO 11359) from the EOS PA 2200 product information sheet; the current EOS sheet no longer lists it."),
        m("peek", "PEEK High-Temp Polymer", 3600.0, 0.38, 100.0, 1.32, 0.6, Some(47.0e-6), Some(0.25),
          "Victrex PEEK 450G / ISO 527 semi-crystalline polyetheretherketone: E = 3.6 GPa, ν = 0.38, σ_y = 100 MPa, ρ = 1.32 g/cm³, CTE = 47 ×10⁻⁶/°C, k = 0.25 W/(m·K)."),
        m("tpu95a", "TPU 95A Flexible Elastomer (FDM)", 65.0, 0.48, 18.0, 1.22, 0.8, Some(150.0e-6), Some(0.19),
          "Thermoplastic polyurethane Shore 95A (NinjaTek Cheetah 95A class): E ≈ 65 MPa, ν = 0.48, σ_y = 18 MPa, ρ = 1.22 g/cm³, CTE = 150 ×10⁻⁶/°C, k = 0.19 W/(m·K)."),
        m("alsi10mg", "AlSi10Mg (LPBF)", 70000.0, 0.33, 230.0, 2.67, 0.4, Some(20e-6), Some(150.0),
          "EOS AlSi10Mg data sheet class values: Rp0.2 ≈ 230–260 MPa (lower bound used), E ≈ 70 GPa, density 2.67 g/cm³. CTE 20 ×10⁻⁶/°C (25–100 °C, ASTM E228)."),
        m("al7075", "Aluminium 7075-T6 Aerospace Alloy", 71700.0, 0.33, 503.0, 2.81, 0.4, Some(23.4e-6), Some(155.0),
          "AMS-QQ-A-250/12 Aluminium 7075-T6 wrought plate/bar: E = 71.7 GPa, ν = 0.33, Rp0.2 = 503 MPa, ρ = 2.81 g/cm³, CTE = 23.4 ×10⁻⁶/°C, k = 155 W/(m·K)."),
        al5754,
        m("ti64", "Ti-6Al-4V (LPBF)", 110000.0, 0.34, 960.0, 4.41, 0.4, Some(9.0e-6), Some(6.7),
          "EOS Ti64 data sheet: E 110 ± 7 GPa, Rp0.2 1030 ± 70 MPa (mean − 1σ used), density 4.41 g/cm³. CTE 9.0 ×10⁻⁶/°C (25–100 °C, ASTM E228) from the EOS Ti64 Grade 23 sheet."),
        m("ss316l", "316L stainless (LPBF)", 180000.0, 0.30, 470.0, 7.98, 0.4, Some(15.7e-6), Some(16.2),
          "EOS StainlessSteel 316L data sheet: E ≈ 180–185 GPa, Rp0.2 ≈ 470–530 MPa (lower bound used), density 7.98 g/cm³. CTE 15.7 ×10⁻⁶/°C (25–100 °C, ASTM E228)."),
        dc04,
        m("cucrzr", "CuCrZr copper alloy (LPBF)", 128000.0, 0.34, 420.0, 8.91, 0.4, Some(17.0e-6), Some(320.0),
          "Age-hardened CuCrZr LPBF data sheet class values: E ≈ 128 GPa, Rp0.2 ≈ 420 MPa, density 8.91 g/cm³. CTE 17.0 ×10⁻⁶/°C (20–300 °C, ASTM E228)."),
        m_ortho("cf_woven_3k", "3K Woven CFRP Laminate (Space Epoxy, Tsai–Wu)", 70000.0, 0.10, 620.0, 1.58, 0.6, Some(2.1e-6), Some(5.0), Some(42.0), Some(58.0),
          "Aerospace 3K plain-weave carbon/epoxy quasi-balanced laminate (AGATE / T300-Cycom 934 class): in-plane E₁₁ = E₂₂ = 70 GPa, ν₁₂ = 0.10, in-plane tensile/bearing strength 620 MPa, out-of-plane interlaminar tensile S_z = 42 MPa, interlaminar shear S_s = 58 MPa (ASTM D2344), CTE = 2.1 ×10⁻⁶/°C."),
        sic,
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
