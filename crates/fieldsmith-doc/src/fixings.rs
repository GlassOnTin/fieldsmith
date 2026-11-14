//! Built-in catalogue of engineering fixings (self-tapping screws, hex head screws,
//! hex cap head bolts, washers, hex nuts, nyloc lock nuts, and cantilever hook-clip
//! plastic box snap-fits), automatic multi-face host-object drilling/tapping/counterboring,
//! stack inspection & in-place reconfiguration, and a-priori + empirical torque envelope &
//! Weibull/creep failure distribution fitting.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::f64::consts::PI;

use crate::materials::{self, Material, MaterialRole};
use crate::model::{
    Consumer, Document, Face, Let, Load, LoadKind, Node, NodeKind, Op, ProfileVertex, Region,
    Selector, Support, Transform, UncertaintySpec,
};
use crate::param;

/// Engineering fixing family in the built-in geometry catalogue.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixingKind {
    /// Thread-forming / self-tapping screw (Plastite 30°/45° or ISO 1478) with automatic
    /// pilot hole, lead-in chamfer, thread-bite rings, and optional reinforcing boss.
    #[default]
    #[serde(alias = "selftapping", alias = "self_tapping", alias = "plastite")]
    SelfTappingScrew,
    /// ISO 4017 full-thread hexagonal head screw with 6-sided extruded hex head and
    /// automatic clearance + tapped thread bore in the fixed object.
    #[serde(alias = "hex_screw", alias = "iso4017")]
    HexHeadScrew,
    /// ISO 4762 (DIN 912) socket head cap screw with cylindrical head, internal 6-sided
    /// Allen hex drive recess, and automatic counterbore pocket + tapped bore.
    #[serde(
        alias = "socket_head_cap",
        alias = "cap_bolt",
        alias = "iso4762",
        alias = "din912"
    )]
    HexCapBolt,
    /// ISO 7089 (DIN 125) structural flat / Belleville load-spreading washer with
    /// automatic spotface seating recess and bolt bore.
    #[serde(alias = "flat_washer", alias = "iso7089")]
    Washer,
    /// ISO 4032 6-sided hexagonal nut with automatic hex nut-trap pocket and bolt clearance hole.
    #[serde(alias = "nut", alias = "iso4032")]
    HexNut,
    /// ISO 10511 prevailing-torque Nyloc hex lock nut with domed collar and PA12 nylon
    /// locking insert ring.
    #[serde(alias = "nyloc", alias = "lock_nut", alias = "iso10511")]
    NylocNut,
    /// Parametric cantilever hook-clip snap-fit latch & automatic undercut catch slot
    /// for plastic box / instrument enclosure designs.
    #[serde(alias = "hook_clip", alias = "snap_fit", alias = "cantilever_snap")]
    HookClipBox,
}

impl FixingKind {
    pub const ALL: [FixingKind; 7] = [
        FixingKind::SelfTappingScrew,
        FixingKind::HexHeadScrew,
        FixingKind::HexCapBolt,
        FixingKind::Washer,
        FixingKind::HexNut,
        FixingKind::NylocNut,
        FixingKind::HookClipBox,
    ];

    pub fn id(self) -> &'static str {
        match self {
            FixingKind::SelfTappingScrew => "self_tapping_screw",
            FixingKind::HexHeadScrew => "hex_head_screw",
            FixingKind::HexCapBolt => "hex_cap_bolt",
            FixingKind::Washer => "washer",
            FixingKind::HexNut => "hex_nut",
            FixingKind::NylocNut => "nyloc_nut",
            FixingKind::HookClipBox => "hook_clip_box",
        }
    }

    pub fn is_bolt_or_screw(self) -> bool {
        matches!(
            self,
            FixingKind::SelfTappingScrew | FixingKind::HexHeadScrew | FixingKind::HexCapBolt
        )
    }

    pub fn parse(s: &str) -> Option<FixingKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "self_tapping_screw" | "selftapping" | "self_tapping" | "plastite" => {
                Some(FixingKind::SelfTappingScrew)
            }
            "hex_head_screw" | "hex_screw" | "iso4017" => Some(FixingKind::HexHeadScrew),
            "hex_cap_bolt" | "socket_head_cap" | "cap_bolt" | "iso4762" | "din912" => {
                Some(FixingKind::HexCapBolt)
            }
            "washer" | "flat_washer" | "iso7089" => Some(FixingKind::Washer),
            "hex_nut" | "nut" | "iso4032" => Some(FixingKind::HexNut),
            "nyloc_nut" | "nyloc" | "lock_nut" | "iso10511" => Some(FixingKind::NylocNut),
            "hook_clip_box" | "hook_clip" | "snap_fit" | "cantilever_snap" => {
                Some(FixingKind::HookClipBox)
            }
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FixingKind::SelfTappingScrew => "Self-Tapping Screw (Plastite / ISO 1478)",
            FixingKind::HexHeadScrew => "Hex Head Screw (ISO 4017)",
            FixingKind::HexCapBolt => "Hex Cap Head Bolt (ISO 4762 / DIN 912)",
            FixingKind::Washer => "Structural Washer (ISO 7089)",
            FixingKind::HexNut => "Hex Nut (ISO 4032)",
            FixingKind::NylocNut => "Nyloc Lock Nut (ISO 10511)",
            FixingKind::HookClipBox => "Hook-Clip Plastic Box Snap-Fit",
        }
    }
}

/// Optional mated nut associated with a bolt or machine screw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssociatedNutKind {
    #[default]
    None,
    HexNut,
    NylocNut,
}

/// Host-face placement pattern mode for fixing insertion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixingPlacementMode {
    #[default]
    Edge,
    Center,
    #[serde(
        rename = "flange_4_corners",
        alias = "flange4_corners",
        alias = "flange4corners"
    )]
    Flange4Corners,
    BoltCircle,
    SketchPoints,
}

/// Explicit 2D `(u, v)` coordinate on a host surface frame (mm).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PointUV {
    pub u: f64,
    pub v: f64,
}

/// Standard metric fixing dimensions (mm).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FixingSizeSpec {
    /// Designation such as `"M2.5"`, `"M3"`, `"M4"`, `"M5"`, `"M6"`, `"M8"`.
    pub name: String,
    /// Nominal major thread diameter `d` (mm).
    pub major_d: f64,
    /// Thread pitch `P` (mm).
    pub pitch: f64,
    /// Recommended self-tapping pilot hole diameter in thermoplastics `d_pilot,plastic` (mm).
    pub pilot_d_plastic: f64,
    /// Recommended tapped / self-tapping pilot hole diameter in metals `d_pilot,metal` (mm).
    pub pilot_d_metal: f64,
    /// Standard close-fit clearance hole diameter `d_clear` (mm).
    pub clearance_d: f64,
    /// Across-flats hex wrench width `s` (mm) for hex heads and hex nuts.
    pub hex_flats_s: f64,
    /// Socket cap head outer diameter `d_k` (mm).
    pub cap_head_d: f64,
    /// Head height `k` (mm).
    pub head_h: f64,
    /// Internal Allen hex key across-flats `s_key` (mm) for socket cap bolts.
    pub allen_s: f64,
    /// Standard flat washer outer diameter `d_w` (mm).
    pub washer_od: f64,
    /// Standard flat washer thickness `t_w` (mm).
    pub washer_t: f64,
    /// Standard hex nut thickness `m_nut` (mm).
    pub nut_h: f64,
    /// Recommended plastic boss outer diameter `D_boss ≈ 2.5 d` (mm).
    pub boss_od: f64,
}

pub fn fixing_sizes() -> Vec<FixingSizeSpec> {
    vec![
        FixingSizeSpec {
            name: "M2.5".into(),
            major_d: 2.5,
            pitch: 0.45,
            pilot_d_plastic: 2.00,
            pilot_d_metal: 2.05,
            clearance_d: 2.75,
            hex_flats_s: 5.0,
            cap_head_d: 4.5,
            head_h: 2.5,
            allen_s: 2.0,
            washer_od: 6.0,
            washer_t: 0.5,
            nut_h: 2.0,
            boss_od: 6.25,
        },
        FixingSizeSpec {
            name: "M3".into(),
            major_d: 3.0,
            pitch: 0.50,
            pilot_d_plastic: 2.40,
            pilot_d_metal: 2.50,
            clearance_d: 3.40,
            hex_flats_s: 5.5,
            cap_head_d: 5.5,
            head_h: 3.0,
            allen_s: 2.5,
            washer_od: 7.0,
            washer_t: 0.5,
            nut_h: 2.4,
            boss_od: 7.50,
        },
        FixingSizeSpec {
            name: "M4".into(),
            major_d: 4.0,
            pitch: 0.70,
            pilot_d_plastic: 3.20,
            pilot_d_metal: 3.30,
            clearance_d: 4.50,
            hex_flats_s: 7.0,
            cap_head_d: 7.0,
            head_h: 4.0,
            allen_s: 3.0,
            washer_od: 9.0,
            washer_t: 0.8,
            nut_h: 3.2,
            boss_od: 10.0,
        },
        FixingSizeSpec {
            name: "M5".into(),
            major_d: 5.0,
            pitch: 0.80,
            pilot_d_plastic: 4.05,
            pilot_d_metal: 4.20,
            clearance_d: 5.50,
            hex_flats_s: 8.0,
            cap_head_d: 8.5,
            head_h: 5.0,
            allen_s: 4.0,
            washer_od: 10.0,
            washer_t: 1.0,
            nut_h: 4.7,
            boss_od: 12.5,
        },
        FixingSizeSpec {
            name: "M6".into(),
            major_d: 6.0,
            pitch: 1.00,
            pilot_d_plastic: 4.90,
            pilot_d_metal: 5.00,
            clearance_d: 6.60,
            hex_flats_s: 10.0,
            cap_head_d: 10.0,
            head_h: 6.0,
            allen_s: 5.0,
            washer_od: 12.0,
            washer_t: 1.6,
            nut_h: 5.2,
            boss_od: 15.0,
        },
        FixingSizeSpec {
            name: "M8".into(),
            major_d: 8.0,
            pitch: 1.25,
            pilot_d_plastic: 6.60,
            pilot_d_metal: 6.80,
            clearance_d: 9.00,
            hex_flats_s: 13.0,
            cap_head_d: 13.0,
            head_h: 8.0,
            allen_s: 6.0,
            washer_od: 16.0,
            washer_t: 1.6,
            nut_h: 6.8,
            boss_od: 20.0,
        },
    ]
}

pub fn find_fixing_size(name: &str) -> FixingSizeSpec {
    let norm = name.trim().to_ascii_uppercase();
    fixing_sizes()
        .into_iter()
        .find(|s| s.name == norm)
        .unwrap_or_else(|| {
            fixing_sizes()
                .into_iter()
                .find(|s| s.name == "M4")
                .expect("M4 size exists")
        })
}

fn find_fixing_size_by_diameter(nominal_d_mm: f64) -> FixingSizeSpec {
    fixing_sizes()
        .into_iter()
        .find(|s| (s.major_d - nominal_d_mm).abs() < 0.35)
        .unwrap_or_else(|| find_fixing_size("M4"))
}

/// Target substrate material regime for automatic failure mode selection & hole sizing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubstrateRegime {
    /// Thermoplastic boss / housing (PETG, PLA, ABS, PA12, PA66, PC, POM, PEEK): governed by
    /// viscoplastic boss creep relaxation, Lamé hoop boss splitting, or polymer thread shear stripping.
    PlasticBoss,
    /// Ductile metal housing (AlSi10Mg, Ti-6Al-4V, SS316L, Steel 8.8/10.9/12.9, Inconel 718):
    /// governed by combined axial-torsional bolt yield or internal thread shear stripping.
    MetalHousing,
    /// Brittle / ceramic / cryogenic optical structure (SiC, Al₂O₃, N-BK7 glass):
    /// governed by 2-parameter Weibull principal stress volume/surface fracture statistics.
    BrittleWeibullOptics,
    /// Anisotropic woven composite laminate (`cf_woven_3k`, `fr4_g10`):
    /// governed by out-of-plane interlaminar Tsai–Wu peel / bearing delamination.
    WovenComposite,
}

impl SubstrateRegime {
    pub fn classify_material(mat_id: &str) -> Self {
        match mat_id.trim().to_ascii_lowercase().as_str() {
            "pla" | "petg" | "abs" | "pa12" | "nylon_pa66" | "pc_lexan" | "pom_delrin" | "peek"
            | "peek_cf30" | "ptfe_teflon" | "tpu95a" => SubstrateRegime::PlasticBoss,
            "cf_woven_3k" | "fr4_g10" | "cfrp" | "woven_composite" => {
                SubstrateRegime::WovenComposite
            }
            "sic_jwst" | "alumina_al2o3" | "nbk7_glass" | "soda_lime_glass" | "sic_refractory"
            | "ceramic" | "beryllium" => SubstrateRegime::BrittleWeibullOptics,
            _ => SubstrateRegime::MetalHousing,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SubstrateRegime::PlasticBoss => {
                "Thermoplastic Boss (Viscoplastic Creep + Boss Hoop Split + Thread Strip)"
            }
            SubstrateRegime::MetalHousing => {
                "Ductile Metal Housing (Combined Tension–Torsion Yield + Thread Shear Strip)"
            }
            SubstrateRegime::BrittleWeibullOptics => {
                "Brittle / Space-Optics Substrate (Weibull Principal-Stress Fracture P_f)"
            }
            SubstrateRegime::WovenComposite => {
                "Anisotropic Woven Composite (Tsai–Wu Interlaminar Peel & Bearing Delamination)"
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn extended_fixing_material(
    id: &str,
    name: &str,
    e: f64,
    nu: f64,
    yield_strength: f64,
    density: f64,
    alpha: f64,
    yield_z: f64,
    shear_z: f64,
    source: &str,
) -> Material {
    Material {
        id: id.into(),
        name: name.into(),
        e,
        nu,
        yield_strength,
        density,
        min_wall: 0.6,
        alpha: Some(alpha),
        k: None,
        cp: None,
        yield_z: Some(yield_z),
        shear_z: Some(shear_z),
        hardening_n: None,
        hardening_k: None,
        weibull_m: None,
        role: MaterialRole::Structural,
        source: source.into(),
    }
}

/// Extended ISO 898-1 / ISO 3506-1 / Aerospace / Polymer / Ceramic fastener and housing materials.
pub fn extended_fixing_materials() -> Vec<Material> {
    vec![
        extended_fixing_material(
            "steel_8_8",
            "Steel Class 8.8 Medium-Carbon Q&T",
            210_000.0,
            0.29,
            640.0,
            7.85,
            11.5e-6,
            384.0,
            369.5,
            "ISO 898-1 Property Class 8.8 quenched and tempered steel fastener: E=210 GPa, Rp0.2=640 MPa, Rm=800 MPa.",
        ),
        extended_fixing_material(
            "steel_10_9",
            "Steel Class 10.9 Alloy High-Tensile",
            210_000.0,
            0.29,
            900.0,
            7.85,
            11.5e-6,
            540.0,
            519.6,
            "ISO 898-1 Property Class 10.9 alloy steel fastener: E=210 GPa, Rp0.2=900 MPa, Rm=1040 MPa.",
        ),
        extended_fixing_material(
            "steel_12_9",
            "Steel Class 12.9 Socket Cap CrMo Alloy",
            210_000.0,
            0.29,
            1080.0,
            7.85,
            11.5e-6,
            648.0,
            623.5,
            "ISO 898-1 Property Class 12.9 CrMo socket head cap screw: E=210 GPa, Rp0.2=1080 MPa, Rm=1220 MPa.",
        ),
        extended_fixing_material(
            "steel_4_8",
            "Steel Class 4.8 Mild Carbon Steel",
            205_000.0,
            0.29,
            320.0,
            7.85,
            12.0e-6,
            192.0,
            184.8,
            "ISO 898-1 Property Class 4.8 low-carbon steel screw: E=205 GPa, ReL=320 MPa, Rm=420 MPa.",
        ),
        extended_fixing_material(
            "steel_1020",
            "AISI 1020 Structural Mild Steel",
            205_000.0,
            0.29,
            350.0,
            7.85,
            12.0e-6,
            210.0,
            202.1,
            "AISI 1020 cold-drawn carbon steel: E=205 GPa, Rp0.2=350 MPa.",
        ),
        extended_fixing_material(
            "ss304_a2_70",
            "A2-70 / 304 Stainless Steel",
            193_000.0,
            0.29,
            450.0,
            7.93,
            17.2e-6,
            270.0,
            259.8,
            "ISO 3506-1 Grade A2-70 cold-worked 18/8 austenitic stainless fastener: E=193 GPa, Rp0.2=450 MPa, Rm=700 MPa.",
        ),
        extended_fixing_material(
            "steel_304",
            "AISI 304 / A2-70 Stainless Steel",
            193_000.0,
            0.29,
            450.0,
            7.93,
            17.2e-6,
            270.0,
            259.8,
            "ISO 3506-1 Grade A2-70 / AISI 304 austenitic stainless fastener: E=193 GPa, Rp0.2=450 MPa.",
        ),
        extended_fixing_material(
            "ss17_4ph",
            "17-4 PH (H900) Stainless Steel",
            197_000.0,
            0.27,
            1170.0,
            7.78,
            10.8e-6,
            702.0,
            675.5,
            "AMS 5643 17-4 PH Condition H900 aerospace stainless bolt: E=197 GPa, Rp0.2=1170 MPa.",
        ),
        extended_fixing_material(
            "in718",
            "Inconel 718 High-Temp Superalloy",
            190_000.0,
            0.29,
            980.0,
            8.19,
            13.0e-6,
            588.0,
            565.8,
            "AMS 5662 Inconel 718 precipitation-hardened bolt: E=190 GPa, Rp0.2=980 MPa.",
        ),
        extended_fixing_material(
            "a286_superalloy",
            "A-286 Iron-Nickel Superalloy Bolt",
            201_000.0,
            0.30,
            655.0,
            7.92,
            16.5e-6,
            393.0,
            378.2,
            "AMS 5732 A-286 cryogenic & turbine fastener alloy: E=201 GPa, Rp0.2=655 MPa.",
        ),
        extended_fixing_material(
            "al7075_t6",
            "Aluminium 7075-T6 Aerospace Alloy",
            71_700.0,
            0.33,
            503.0,
            2.81,
            23.4e-6,
            301.8,
            290.4,
            "AA 7075-T6 aerospace aluminium fastener: E=71.7 GPa, Rp0.2=503 MPa, Rm=572 MPa.",
        ),
        extended_fixing_material(
            "brass_cuZn39pb3",
            "Brass CW614N / CuZn39Pb3",
            97_000.0,
            0.34,
            260.0,
            8.47,
            20.5e-6,
            156.0,
            150.1,
            "EN 12164 CW614N (CuZn39Pb3 / C36000) free-machining brass fastener: E=97 GPa, Rp0.2=260 MPa.",
        ),
        extended_fixing_material(
            "bronze_c65100",
            "Silicon Bronze C65100 Marine Fastener",
            105_000.0,
            0.34,
            380.0,
            8.75,
            18.0e-6,
            228.0,
            219.4,
            "ASTM F468 C65100 Silicon Bronze cold-headed bolt: E=105 GPa, Rp0.2=380 MPa.",
        ),
        extended_fixing_material(
            "az91d_mg",
            "Magnesium AZ91D Die-Cast Housing",
            45_000.0,
            0.35,
            160.0,
            1.81,
            26.0e-6,
            96.0,
            92.4,
            "ASTM AZ91D die-cast magnesium enclosure: E=45 GPa, Rp0.2=160 MPa.",
        ),
        extended_fixing_material(
            "nylon_pa66",
            "PA66 Nylon 6/6 Molded Insulating Fastener",
            2_800.0,
            0.39,
            75.0,
            1.14,
            80.0e-6,
            45.0,
            43.3,
            "DIN 34812 PA66 Nylon 6/6 dry-as-molded metric screw/nut: E=2.8 GPa, yield=75 MPa.",
        ),
        extended_fixing_material(
            "peek_cf30",
            "PEEK Aerospace High-Temp Polymer",
            3_900.0,
            0.38,
            100.0,
            1.32,
            47.0e-6,
            60.0,
            57.7,
            "Victrex PEEK 450G high-temperature polymer screw/housing: E=3.9 GPa, yield=100 MPa.",
        ),
        extended_fixing_material(
            "pc_lexan",
            "Polycarbonate (PC / Lexan)",
            2_400.0,
            0.37,
            62.0,
            1.20,
            65.0e-6,
            37.2,
            35.8,
            "Lexan / Makrolon Polycarbonate enclosure & fastener: E=2.4 GPa, yield=62 MPa.",
        ),
        extended_fixing_material(
            "pom_delrin",
            "POM Acetal (Delrin)",
            3_100.0,
            0.35,
            68.0,
            1.41,
            85.0e-6,
            40.8,
            39.3,
            "DuPont Delrin 100P POM homopolymer: E=3.1 GPa, yield=68 MPa.",
        ),
        extended_fixing_material(
            "fr4_g10",
            "FR-4 / G10 Woven Glass-Epoxy Laminate",
            24_000.0,
            0.14,
            310.0,
            1.85,
            14.0e-6,
            38.0,
            45.0,
            "NEMA FR-4 / G10 woven E-glass epoxy laminate: E=24 GPa, S_z=38 MPa, ILSS=45 MPa.",
        ),
        extended_fixing_material(
            "alumina_al2o3",
            "Alumina Al2O3 99.5% Technical Ceramic",
            370_000.0,
            0.22,
            280.0,
            3.89,
            8.1e-6,
            168.0,
            161.7,
            "99.5% sintered Alumina (Al2O3) technical ceramic: E=370 GPa, flexural scale=280 MPa.",
        ),
        extended_fixing_material(
            "nbk7_glass",
            "N-BK7 Borosilicate Optical Glass",
            82_000.0,
            0.21,
            70.0,
            2.51,
            7.1e-6,
            42.0,
            40.4,
            "Schott N-BK7 optical borosilicate glass: E=82 GPa, Weibull scale=70 MPa.",
        ),
    ]
}

fn find_extended_material(mat_id: &str) -> Option<Material> {
    let trimmed = mat_id.trim();
    if let Some(exact) = extended_fixing_materials()
        .into_iter()
        .find(|m| m.id == trimmed)
    {
        return Some(exact);
    }
    let lower = trimmed.to_ascii_lowercase();
    extended_fixing_materials()
        .into_iter()
        .find(|m| m.id.to_ascii_lowercase() == lower)
        .map(|mut m| {
            m.id = trimmed.to_string();
            m
        })
}

/// Resolve any built-in, document-custom, or extended ISO/aerospace fixing material.
pub fn resolve_fixing_material(
    doc: Option<&Document>,
    mat_id: &str,
    fallback_id: &str,
) -> Material {
    if let Some(d) = doc {
        if let Some(m) = materials::find(d, mat_id) {
            return m;
        }
    }
    let lib = materials::library();
    if let Some(m) = lib.iter().find(|m| m.id == mat_id) {
        return m.clone();
    }
    if let Some(m) = find_extended_material(mat_id) {
        return m;
    }
    lib.into_iter()
        .find(|m| m.id == fallback_id)
        .unwrap_or_default()
}

/// Ensure that `mat_id` is resolvable by `materials::find(doc, mat_id)` so 3D FEA and
/// `Document::validate` accept per-volume fastener and custom substrate materials.
pub fn ensure_fixing_material_in_doc(doc: &mut Document, mat_id: &str) {
    let trimmed = mat_id.trim();
    if trimmed.is_empty() || materials::find(doc, trimmed).is_some() {
        return;
    }
    if let Some(mat) = find_extended_material(trimmed) {
        doc.custom_materials.push(mat);
    }
}

fn catalog_weibull_sigma0_mpa(mat_id: &str, mat: &Material) -> f64 {
    match mat_id.trim() {
        "petg" => 60.0,
        "pla" => 65.0,
        "abs" => 51.0,
        "pa12" => 57.5,
        "nylon_pa66" => 96.0,
        "pc_lexan" => 79.5,
        "pom_delrin" => 87.0,
        "peek" | "peek_cf30" => 128.0,
        "alsi10mg" => 303.0,
        "al7075" | "al7075_t6" => 662.0,
        "steel_4_8" => 422.0,
        "steel_8_8" => 842.0,
        "steel_10_9" => 1185.0,
        "steel_12_9" => 1420.0,
        "ss304_a2_70" | "steel_304" => 592.0,
        "ss316l" => 618.0,
        "ss17_4ph" => 1540.0,
        "ti64" => 1265.0,
        "in718" => 1290.0,
        "a286_superalloy" => 862.0,
        "brass_cuZn39pb3" => 342.0,
        "bronze_c65100" => 500.0,
        "cucrzr" => 553.0,
        "az91d_mg" => 211.0,
        "cf_woven_3k" => 55.0,
        "fr4_g10" => 49.8,
        "sic_jwst" => 340.0,
        "alumina_al2o3" => 280.0,
        "nbk7_glass" => 70.0,
        _ => match SubstrateRegime::classify_material(mat_id) {
            SubstrateRegime::BrittleWeibullOptics => mat.yield_strength,
            SubstrateRegime::WovenComposite => mat.effective_layer_strengths().0 * 1.31,
            SubstrateRegime::MetalHousing => mat.yield_strength * 1.317,
            SubstrateRegime::PlasticBoss => mat.yield_strength * 1.277,
        },
    }
}

/// Build a 6-vertex regular hexagonal profile with across-flats width `s` (mm).
pub fn hex_profile(across_flats_s: f64) -> Vec<ProfileVertex> {
    let r = across_flats_s / 3.0_f64.sqrt();
    (0..6)
        .map(|k| {
            let angle = (k as f64) * PI / 3.0;
            ProfileVertex {
                x: ((r * angle.cos()) * 1000.0).round() / 1000.0,
                y: ((r * angle.sin()) * 1000.0).round() / 1000.0,
                bulge: 0.0,
            }
        })
        .collect()
}

/// Request to insert a catalogue fixing into a `Document` and automatically drill/tap
/// the fixed host object across any host face and pattern mode.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FixingInsertRequest {
    #[serde(default)]
    pub kind: FixingKind,
    /// Size designation (`"M2.5"`, `"M3"`, `"M4"`, `"M5"`, `"M6"`, `"M8"`).
    #[serde(default = "default_size_name")]
    pub size: String,
    /// Engaged thread length or grip thickness `L_eng` (mm).
    #[serde(default = "default_engagement_mm", alias = "engagementMm")]
    pub engagement_mm: f64,
    /// Explicit `[x, y, z]` seating point override (mm). When `None`, resolves on `host_face` of `host_id`.
    #[serde(default)]
    pub at: Option<[f64; 3]>,
    /// Optional ID prefix for generated nodes, regions, lets, and preload loads.
    #[serde(default, alias = "idPrefix")]
    pub id_prefix: Option<String>,
    /// Optional target host node ID (for substrate material classification & surface frame resolution).
    #[serde(default, alias = "hostId")]
    pub host_id: Option<String>,
    /// Target host face (`top`/`pz`, `bottom`/`nz`, `px`/`side`, `nx`, `py`, `ny`). Defaults to `Top`.
    #[serde(default, alias = "hostFace")]
    pub host_face: Option<Face>,
    /// Surface pattern placement mode (`edge`, `center`, `flange_4_corners`, `bolt_circle`, `sketch_points`).
    #[serde(default, alias = "placementMode")]
    pub placement_mode: Option<FixingPlacementMode>,
    /// Distance from `-U` edge of host face (mm) in `edge` mode.
    #[serde(default, alias = "dxEdgeMm")]
    pub dx_edge_mm: Option<f64>,
    /// Distance from `-V` edge of host face (mm) in `edge` mode.
    #[serde(default, alias = "dyEdgeMm")]
    pub dy_edge_mm: Option<f64>,
    /// Offset from host face centre along `U` (mm) in `center` mode.
    #[serde(default, alias = "uOffsetMm")]
    pub u_offset_mm: Option<f64>,
    /// Offset from host face centre along `V` (mm) in `center` mode.
    #[serde(default, alias = "vOffsetMm")]
    pub v_offset_mm: Option<f64>,
    /// Corner inset along `U` from face edges (mm) in `flange_4_corners` mode.
    #[serde(default, alias = "flangeInsetXMm")]
    pub flange_inset_x_mm: Option<f64>,
    /// Corner inset along `V` from face edges (mm) in `flange_4_corners` mode.
    #[serde(default, alias = "flangeInsetYMm")]
    pub flange_inset_y_mm: Option<f64>,
    /// Pitch-circle radius (mm) in `bolt_circle` mode.
    #[serde(default, alias = "boltCircleRadiusMm")]
    pub bolt_circle_radius_mm: Option<f64>,
    /// Bolt count (`2..=16`) in `bolt_circle` mode.
    #[serde(default, alias = "boltCircleCount")]
    pub bolt_circle_count: Option<usize>,
    /// Starting angle (deg) in `bolt_circle` mode.
    #[serde(default, alias = "boltCircleStartDeg")]
    pub bolt_circle_start_deg: Option<f64>,
    /// Explicit 2D `(u, v)` coordinates on host face in `sketch_points` mode.
    #[serde(default, alias = "customPointsUV")]
    pub custom_points_uv: Vec<PointUV>,
    /// Organize each fixing instance into `<inst>_bore` (`subtract`) and `<inst>` (`add`) groups (default `true`).
    #[serde(default, alias = "groupInTree")]
    pub group_in_tree: Option<bool>,
    /// Wrap or append fixing nodes inside the host part's `<host>_assy` group in `doc.root`.
    #[serde(default, alias = "associateWithHostGroup")]
    pub associate_with_host_group: bool,
    /// Bind single-fixing surface coordinates (`_dx`/`_dy` or `_u`/`_v`) to parametric `let` consumers (default `true` when `at` is `None`).
    #[serde(default, alias = "bindSurfaceLets")]
    pub bind_surface_lets: Option<bool>,
    /// Include an ISO 7089 / DIN 125A bearing washer under a bolt/screw head.
    #[serde(default, alias = "includeWasher")]
    pub include_washer: bool,
    /// Associate a mated Hex Nut (ISO 4032) or Nyloc Lock Nut (ISO 10511) with a bolt/screw.
    #[serde(default, alias = "associatedNut")]
    pub associated_nut: AssociatedNutKind,
    /// Optional epistemic preload scatter coefficient of variation `CV = σ_F / μ_F` (e.g. `0.15` for ISO 16047 wrench K-factor scatter)
    /// to register an `UncertaintySpec` on `<prefix>_preload_n` for `O(1)`-solve 3D UQ.
    #[serde(default, alias = "preloadCv")]
    pub preload_cv: Option<f64>,
    /// Fastener hardware material ID (default `"ti64"`).
    #[serde(default = "default_fastener_material", alias = "fastenerMaterial")]
    pub fastener_material: String,
    /// Optional override for substrate material ID; when omitted, uses `doc.material`.
    #[serde(default, alias = "substrateMaterial")]
    pub substrate_material: Option<String>,
    /// External vibration / shock acceleration in g's (`1 g = 9.80665 m/s²`) for automatic
    /// minimal torque & preload calculation.
    #[serde(default = "default_vibration_g", alias = "vibrationG")]
    pub vibration_g: f64,
    /// Supported payload mass per joint (kg) for vibration/knock slip calculation.
    #[serde(default = "default_payload_mass_kg", alias = "payloadMassKg")]
    pub payload_mass_kg: f64,
    /// Optional load case name for the generated bolt preload (`"default"` when omitted).
    #[serde(default, alias = "loadCase")]
    pub load_case: Option<String>,
}

impl Default for FixingInsertRequest {
    fn default() -> Self {
        Self {
            kind: FixingKind::SelfTappingScrew,
            size: default_size_name(),
            engagement_mm: default_engagement_mm(),
            at: None,
            id_prefix: None,
            host_id: None,
            host_face: None,
            placement_mode: None,
            dx_edge_mm: None,
            dy_edge_mm: None,
            u_offset_mm: None,
            v_offset_mm: None,
            flange_inset_x_mm: None,
            flange_inset_y_mm: None,
            bolt_circle_radius_mm: None,
            bolt_circle_count: None,
            bolt_circle_start_deg: None,
            custom_points_uv: Vec::new(),
            group_in_tree: None,
            associate_with_host_group: false,
            bind_surface_lets: None,
            include_washer: false,
            associated_nut: AssociatedNutKind::None,
            preload_cv: None,
            fastener_material: default_fastener_material(),
            substrate_material: None,
            vibration_g: default_vibration_g(),
            payload_mass_kg: default_payload_mass_kg(),
            load_case: None,
        }
    }
}

fn default_size_name() -> String {
    "M4".into()
}
fn default_engagement_mm() -> f64 {
    8.0
}
fn default_fastener_material() -> String {
    "ti64".into()
}
fn default_vibration_g() -> f64 {
    12.0
}
fn default_payload_mass_kg() -> f64 {
    0.15
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FixingInsertSummary {
    pub kind: FixingKind,
    pub size: String,
    pub id_prefix: String,
    pub first_new_id: String,
    pub bore_group_id: String,
    pub created_group_ids: Vec<String>,
    pub host_features_added: Vec<String>,
    pub hardware_nodes_added: Vec<String>,
    pub regions_added: Vec<String>,
    pub loads_added: Vec<String>,
    pub lets_added: Vec<String>,
    pub envelope: FixingEnvelopeReport,
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

fn fmt_num(v: f64) -> String {
    let v = if v == 0.0 { 0.0 } else { v };
    format!("{v}")
}

fn upsert_let(doc: &mut Document, name: &str, val: f64) {
    let val_round = round4(val);
    let expr = fmt_num(val_round);
    if let Some(existing) = doc.lets.iter_mut().find(|l| l.name == name) {
        existing.expr = expr;
        existing.value = val_round;
    } else {
        doc.lets.push(Let {
            name: name.into(),
            expr,
            value: val_round,
        });
    }
}

fn upsert_consumer(
    doc: &mut Document,
    target: &str,
    attr: &str,
    comp: Option<usize>,
    expr: String,
) {
    if let Some(existing) = doc
        .consumed
        .iter_mut()
        .find(|c| c.target == target && c.attr == attr && c.comp == comp)
    {
        existing.expr = expr;
    } else {
        doc.consumed.push(Consumer {
            target: target.into(),
            attr: attr.into(),
            comp,
            expr,
        });
    }
}

#[derive(Clone, Debug)]
struct HostSurfaceFrame {
    face_center: [f64; 3],
    outward_normal: [f64; 3],
    u_axis: [f64; 3],
    v_axis: [f64; 3],
    u_axis_index: usize,
    v_axis_index: usize,
    face_width: f64,
    face_height: f64,
    rotate_deg: [f64; 3],
}

impl HostSurfaceFrame {
    fn point_from_uv(&self, u: f64, v: f64, n_off: f64) -> [f64; 3] {
        [
            round3(
                self.face_center[0]
                    + self.u_axis[0] * u
                    + self.v_axis[0] * v
                    + self.outward_normal[0] * n_off,
            ),
            round3(
                self.face_center[1]
                    + self.u_axis[1] * u
                    + self.v_axis[1] * v
                    + self.outward_normal[1] * n_off,
            ),
            round3(
                self.face_center[2]
                    + self.u_axis[2] * u
                    + self.v_axis[2] * v
                    + self.outward_normal[2] * n_off,
            ),
        ]
    }
}

fn transform_world_point(local: [f64; 3], tr: &Transform) -> [f64; 3] {
    let [rx, ry, rz] = [
        tr.rotate[0].to_radians(),
        tr.rotate[1].to_radians(),
        tr.rotate[2].to_radians(),
    ];
    let (sx, cx) = rx.sin_cos();
    let (sy, cy) = ry.sin_cos();
    let (sz, cz) = rz.sin_cos();
    // R = Rz * Ry * Rx
    let [x, y, z] = local;
    let [x1, y1, z1] = [x, y * cx - z * sx, y * sx + z * cx];
    let [x2, y2, z2] = [x1 * cy + z1 * sy, y1, -x1 * sy + z1 * cy];
    let [x3, y3, z3] = [x2 * cz - y2 * sz, x2 * sz + y2 * cz, z2];
    [
        x3 + tr.translate[0],
        y3 + tr.translate[1],
        z3 + tr.translate[2],
    ]
}

fn aabb_corners(lo: [f64; 3], hi: [f64; 3], tr: &Transform) -> Vec<[f64; 3]> {
    let mut out = Vec::with_capacity(8);
    for &x in &[lo[0], hi[0]] {
        for &y in &[lo[1], hi[1]] {
            for &z in &[lo[2], hi[2]] {
                out.push(transform_world_point([x, y, z], tr));
            }
        }
    }
    out
}

fn node_world_sample_points(node: &Node) -> Vec<[f64; 3]> {
    match &node.kind {
        NodeKind::Box { size, .. } => {
            let [hx, hy, hz] = [size[0] * 0.5, size[1] * 0.5, size[2] * 0.5];
            aabb_corners([-hx, -hy, -hz], [hx, hy, hz], &node.transform)
        }
        NodeKind::Cylinder { radius, height, .. } => aabb_corners(
            [-*radius, -*radius, -height * 0.5],
            [*radius, *radius, height * 0.5],
            &node.transform,
        ),
        NodeKind::Sphere { radius } => aabb_corners(
            [-*radius, -*radius, -*radius],
            [*radius, *radius, *radius],
            &node.transform,
        ),
        NodeKind::Capsule { radius, length } => {
            let hz = length * 0.5 + radius;
            aabb_corners(
                [-*radius, -*radius, -hz],
                [*radius, *radius, hz],
                &node.transform,
            )
        }
        NodeKind::Torus { major, minor } => {
            let r = major + minor;
            aabb_corners([-r, -r, -*minor], [r, r, *minor], &node.transform)
        }
        NodeKind::Extrude { profile, height } => {
            let hz = height * 0.5;
            if profile.is_empty() {
                aabb_corners([-5.0, -5.0, -hz], [5.0, 5.0, hz], &node.transform)
            } else {
                let mut pts = Vec::with_capacity(profile.len() * 2);
                for p in profile {
                    pts.push(transform_world_point([p.x, p.y, -hz], &node.transform));
                    pts.push(transform_world_point([p.x, p.y, hz], &node.transform));
                }
                pts
            }
        }
        NodeKind::Revolve { profile, .. } => {
            if profile.is_empty() {
                aabb_corners([-5.0, -5.0, -5.0], [5.0, 5.0, 5.0], &node.transform)
            } else {
                let mut r_max = 0.0_f64;
                let mut z_min = f64::INFINITY;
                let mut z_max = f64::NEG_INFINITY;
                for p in profile {
                    r_max = r_max.max(p.x.abs());
                    z_min = z_min.min(p.y);
                    z_max = z_max.max(p.y);
                }
                r_max = r_max.max(1.0);
                if (z_max - z_min) < 1e-6 {
                    z_min -= 2.5;
                    z_max += 2.5;
                }
                aabb_corners([-r_max, -r_max, z_min], [r_max, r_max, z_max], &node.transform)
            }
        }
        NodeKind::Mesh { bbox, .. } => aabb_corners(
            [bbox[0], bbox[1], bbox[2]],
            [bbox[3], bbox[4], bbox[5]],
            &node.transform,
        ),
        NodeKind::Group { children, .. } => {
            let mut pts = Vec::new();
            for c in children {
                if !c.suppressed && c.op == Op::Add {
                    for p in node_world_sample_points(c) {
                        pts.push(transform_world_point(p, &node.transform));
                    }
                }
            }
            if pts.is_empty() {
                aabb_corners([-20.0, -20.0, -10.0], [20.0, 20.0, 10.0], &node.transform)
            } else {
                pts
            }
        }
        _ => aabb_corners([-20.0, -20.0, -10.0], [20.0, 20.0, 10.0], &node.transform),
    }
}

fn resolve_host_surface_frame(
    doc: &Document,
    host_id: Option<&str>,
    face: Face,
) -> HostSurfaceFrame {
    let host = host_id
        .and_then(|id| doc.find(id))
        .or_else(|| doc.root.iter().find(|n| n.op == Op::Add));
    let effective = match host {
        Some(h)
            if matches!(h.kind, NodeKind::Group { .. }) && h.transform.is_identity() =>
        {
            if let NodeKind::Group { children, .. } = &h.kind {
                children
                    .iter()
                    .find(|c| c.op == Op::Add && !matches!(c.kind, NodeKind::Group { .. }))
                    .unwrap_or(h)
            } else {
                h
            }
        }
        Some(h) => h,
        None => {
            return HostSurfaceFrame {
                face_center: [0.0, 0.0, 5.0],
                outward_normal: [0.0, 0.0, 1.0],
                u_axis: [1.0, 0.0, 0.0],
                v_axis: [0.0, 1.0, 0.0],
                u_axis_index: 0,
                v_axis_index: 1,
                face_width: 40.0,
                face_height: 40.0,
                rotate_deg: [0.0, 0.0, 0.0],
            };
        }
    };

    let pts = node_world_sample_points(effective);
    let mut min_w = [f64::INFINITY; 3];
    let mut max_w = [f64::NEG_INFINITY; 3];
    for p in &pts {
        for k in 0..3 {
            min_w[k] = min_w[k].min(p[k]);
            max_w[k] = max_w[k].max(p[k]);
        }
    }

    let (
        outward_normal,
        u_axis,
        v_axis,
        u_axis_index,
        v_axis_index,
        n_axis_index,
        n_sign,
        rotate_deg,
    ) = match face {
        Face::Top | Face::Pz | Face::All => (
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            0,
            1,
            2,
            1.0_f64,
            [0.0, 0.0, 0.0],
        ),
        Face::Bottom | Face::Nz => (
            [0.0, 0.0, -1.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            0,
            1,
            2,
            -1.0_f64,
            [180.0, 0.0, 0.0],
        ),
        Face::Px | Face::Side => (
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            1,
            2,
            0,
            1.0_f64,
            [0.0, 90.0, 0.0],
        ),
        Face::Nx => (
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            1,
            2,
            0,
            -1.0_f64,
            [0.0, -90.0, 0.0],
        ),
        Face::Py => (
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            0,
            2,
            1,
            1.0_f64,
            [-90.0, 0.0, 0.0],
        ),
        Face::Ny => (
            [0.0, -1.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            0,
            2,
            1,
            -1.0_f64,
            [90.0, 0.0, 0.0],
        ),
    };

    let n_coord = if n_sign > 0.0 {
        max_w[n_axis_index]
    } else {
        min_w[n_axis_index]
    };
    let mut f_u_min = f64::INFINITY;
    let mut f_u_max = f64::NEG_INFINITY;
    let mut f_v_min = f64::INFINITY;
    let mut f_v_max = f64::NEG_INFINITY;
    for p in &pts {
        if (p[n_axis_index] - n_coord).abs() <= 1e-3 {
            f_u_min = f_u_min.min(p[u_axis_index]);
            f_u_max = f_u_max.max(p[u_axis_index]);
            f_v_min = f_v_min.min(p[v_axis_index]);
            f_v_max = f_v_max.max(p[v_axis_index]);
        }
    }
    let (u_lo, u_hi) = if f_u_max > f_u_min + 1e-3 {
        (f_u_min, f_u_max)
    } else {
        (min_w[u_axis_index], max_w[u_axis_index])
    };
    let (v_lo, v_hi) = if f_v_max > f_v_min + 1e-3 {
        (f_v_min, f_v_max)
    } else {
        (min_w[v_axis_index], max_w[v_axis_index])
    };

    let face_width = (u_hi - u_lo).max(1.0);
    let face_height = (v_hi - v_lo).max(1.0);
    let mut face_center = [0.0; 3];
    face_center[n_axis_index] = n_coord;
    face_center[u_axis_index] = 0.5 * (u_lo + u_hi);
    face_center[v_axis_index] = 0.5 * (v_lo + v_hi);

    HostSurfaceFrame {
        face_center,
        outward_normal,
        u_axis,
        v_axis,
        u_axis_index,
        v_axis_index,
        face_width,
        face_height,
        rotate_deg,
    }
}

fn collect_all_doc_ids(doc: &Document) -> HashSet<String> {
    fn walk_nodes(nodes: &[Node], set: &mut HashSet<String>) {
        for n in nodes {
            set.insert(n.id.clone());
            if let NodeKind::Group { children, .. } = &n.kind {
                walk_nodes(children, set);
            }
        }
    }
    let mut set = HashSet::new();
    walk_nodes(&doc.root, &mut set);
    for l in &doc.lets {
        set.insert(l.name.clone());
    }
    for r in &doc.regions {
        set.insert(r.id.clone());
    }
    for s in &doc.supports {
        set.insert(s.id.clone());
    }
    for ld in &doc.loads {
        set.insert(ld.id.clone());
    }
    for sk in &doc.sketches {
        set.insert(sk.id.clone());
    }
    for sf in &doc.sketch_features {
        set.insert(sf.feature_id.clone());
    }
    set
}

fn fresh_id(taken: &HashSet<String>, base: &str) -> String {
    if !taken.contains(base) {
        return base.to_string();
    }
    let mut k = 1usize;
    loop {
        let cand = format!("{base}_{k}");
        if !taken.contains(&cand) {
            return cand;
        }
        k += 1;
    }
}

fn associate_nodes_with_host(
    doc: &mut Document,
    host_id: Option<&str>,
    child_nodes: Vec<Node>,
    wrap_host_in_assy_group: bool,
) {
    let Some(target_id) = host_id.filter(|s| !s.is_empty()) else {
        doc.root.extend(child_nodes);
        return;
    };
    let taken = collect_all_doc_ids(doc);

    fn insert_recursive(
        list: &mut Vec<Node>,
        in_parent_group: bool,
        target_id: &str,
        child_nodes: &mut Option<Vec<Node>>,
        wrap_host_in_assy_group: bool,
        taken: &HashSet<String>,
    ) -> bool {
        for idx in 0..list.len() {
            if list[idx].id == target_id {
                let Some(kids) = child_nodes.take() else {
                    return true;
                };
                if let NodeKind::Group { children, .. } = &mut list[idx].kind {
                    children.extend(kids);
                    return true;
                }
                if in_parent_group {
                    list.extend(kids);
                    return true;
                }
                if wrap_host_in_assy_group {
                    let host_clone = list[idx].clone();
                    let assy_id = fresh_id(taken, &format!("{}_assy", host_clone.id));
                    let mut group_children = Vec::with_capacity(1 + kids.len());
                    group_children.push(Node {
                        op: Op::Add,
                        ..host_clone.clone()
                    });
                    group_children.extend(kids);
                    list[idx] = Node {
                        id: assy_id,
                        op: host_clone.op,
                        blend: 0.0,
                        suppressed: false,
                        transform: Transform {
                            translate: [0.0; 3],
                            rotate: [0.0; 3],
                        },
                        material: None,
                        color: None,
                        kind: NodeKind::Group {
                            children: group_children,
                            modifiers: Vec::new(),
                        },
                    };
                    return true;
                }
                for (offset, k) in kids.into_iter().enumerate() {
                    list.insert(idx + 1 + offset, k);
                }
                return true;
            }
            if let NodeKind::Group { children, .. } = &mut list[idx].kind {
                if insert_recursive(
                    children,
                    true,
                    target_id,
                    child_nodes,
                    wrap_host_in_assy_group,
                    taken,
                ) {
                    return true;
                }
            }
        }
        false
    }

    let mut opt_kids = Some(child_nodes);
    if !insert_recursive(
        &mut doc.root,
        false,
        target_id,
        &mut opt_kids,
        wrap_host_in_assy_group,
        &taken,
    ) {
        if let Some(rem) = opt_kids {
            doc.root.extend(rem);
        }
    }
}

fn format_offset_expr(base: f64, var_name: &str) -> String {
    let b = round4(base);
    if b.abs() < 1.0e-9 {
        var_name.to_string()
    } else if b > 0.0 {
        format!("{} + {var_name}", fmt_num(b))
    } else {
        format!("{var_name} - {}", fmt_num(-b))
    }
}

/// Insert a fixing from the built-in catalogue into `doc`, automatically creating
/// the mated holes, counterbores, tapped thread bores, washer/nut stacks, or snap-fit
/// undercut slots on the fixed host object, along with hierarchical `<prefix>_bore` (−)
/// and `<prefix>` (+) groups, named regions, `let` consumer bindings, and clamping preload.
pub fn insert_fixing_into_document(
    doc: &mut Document,
    req: &FixingInsertRequest,
) -> FixingInsertSummary {
    let size_spec = find_fixing_size(&req.size);
    let sub_mat_id = req
        .substrate_material
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| doc.material.clone());

    if let Some(sub_override) = req.substrate_material.as_deref() {
        if !sub_override.trim().is_empty() {
            ensure_fixing_material_in_doc(doc, sub_override);
        }
    }
    ensure_fixing_material_in_doc(doc, &req.fastener_material);

    let regime = SubstrateRegime::classify_material(&sub_mat_id);

    // Ensure there is a host body if the document tree is currently empty.
    if doc.root.is_empty() {
        let host_h = (req.engagement_mm + 4.0).max(10.0);
        doc.root.push(Node {
            id: "host_housing".into(),
            op: Op::Add,
            kind: NodeKind::Box {
                size: [32.0, 24.0, host_h],
                round: 1.0,
            },
            transform: Transform {
                translate: [0.0, 0.0, 0.0],
                rotate: [0.0; 3],
            },
            blend: 0.0,
            suppressed: false,
            material: None,
            color: None,
        });
        doc.regions.push(Region {
            id: "housing_mount".into(),
            selectors: vec![Selector {
                shape: "host_housing".into(),
                face: Face::Nz,
                patch: None,
            }],
        });
        doc.supports.push(Support {
            id: "mount_fix".into(),
            region: "housing_mount".into(),
            fixed: [true, true, true],
        });
    }

    let target_host_id = req
        .host_id
        .clone()
        .filter(|id| doc.find(id).is_some())
        .or_else(|| {
            doc.root
                .iter()
                .find(|n| n.op == Op::Add)
                .map(|n| n.id.clone())
        });

    let host_face = req.host_face.unwrap_or(Face::Top);
    let frame = resolve_host_surface_frame(doc, target_host_id.as_deref(), host_face);
    let mode = req.placement_mode.unwrap_or(FixingPlacementMode::Edge);

    #[derive(Clone, Copy)]
    struct ResolvedPlacement {
        surface_pt: [f64; 3],
        u: f64,
        v: f64,
        dx: f64,
        dy: f64,
    }

    let placements: Vec<ResolvedPlacement> = if let Some(explicit_xyz) = req.at {
        vec![ResolvedPlacement {
            surface_pt: explicit_xyz,
            u: 0.0,
            v: 0.0,
            dx: 0.5 * frame.face_width,
            dy: 0.5 * frame.face_height,
        }]
    } else {
        match mode {
            FixingPlacementMode::SketchPoints if !req.custom_points_uv.is_empty() => req
                .custom_points_uv
                .iter()
                .map(|p| ResolvedPlacement {
                    surface_pt: frame.point_from_uv(p.u, p.v, 0.0),
                    u: p.u,
                    v: p.v,
                    dx: 0.5 * frame.face_width + p.u,
                    dy: 0.5 * frame.face_height + p.v,
                })
                .collect(),
            FixingPlacementMode::Flange4Corners => {
                let in_x = req
                    .flange_inset_x_mm
                    .unwrap_or_else(|| (frame.face_width * 0.18).max(5.0))
                    .clamp(
                        size_spec.washer_od * 0.6,
                        (frame.face_width * 0.45).max(size_spec.washer_od * 0.6),
                    );
                let in_y = req
                    .flange_inset_y_mm
                    .unwrap_or_else(|| (frame.face_height * 0.18).max(5.0))
                    .clamp(
                        size_spec.washer_od * 0.6,
                        (frame.face_height * 0.45).max(size_spec.washer_od * 0.6),
                    );
                let u_span = (0.5 * frame.face_width - in_x).max(1.0);
                let v_span = (0.5 * frame.face_height - in_y).max(1.0);
                [
                    (-u_span, -v_span),
                    (u_span, -v_span),
                    (u_span, v_span),
                    (-u_span, v_span),
                ]
                .into_iter()
                .map(|(u, v)| ResolvedPlacement {
                    surface_pt: frame.point_from_uv(u, v, 0.0),
                    u,
                    v,
                    dx: 0.5 * frame.face_width + u,
                    dy: 0.5 * frame.face_height + v,
                })
                .collect()
            }
            FixingPlacementMode::BoltCircle => {
                let n_bolts = req.bolt_circle_count.unwrap_or(4).clamp(2, 16);
                let r_pcd = req
                    .bolt_circle_radius_mm
                    .unwrap_or_else(|| frame.face_width.min(frame.face_height) * 0.32)
                    .max(size_spec.washer_od);
                let start_rad = req.bolt_circle_start_deg.unwrap_or(0.0).to_radians();
                (0..n_bolts)
                    .map(|k| {
                        let ang = start_rad + (2.0 * PI * (k as f64)) / (n_bolts as f64);
                        let u = round3(r_pcd * ang.cos());
                        let v = round3(r_pcd * ang.sin());
                        ResolvedPlacement {
                            surface_pt: frame.point_from_uv(u, v, 0.0),
                            u,
                            v,
                            dx: 0.5 * frame.face_width + u,
                            dy: 0.5 * frame.face_height + v,
                        }
                    })
                    .collect()
            }
            FixingPlacementMode::Center => {
                let u = req.u_offset_mm.unwrap_or(0.0);
                let v = req.v_offset_mm.unwrap_or(0.0);
                vec![ResolvedPlacement {
                    surface_pt: frame.point_from_uv(u, v, 0.0),
                    u,
                    v,
                    dx: 0.5 * frame.face_width + u,
                    dy: 0.5 * frame.face_height + v,
                }]
            }
            FixingPlacementMode::Edge | FixingPlacementMode::SketchPoints => {
                let dx = req.dx_edge_mm.unwrap_or(0.5 * frame.face_width);
                let dy = req.dy_edge_mm.unwrap_or(0.5 * frame.face_height);
                let u = -0.5 * frame.face_width + dx;
                let v = -0.5 * frame.face_height + dy;
                vec![ResolvedPlacement {
                    surface_pt: frame.point_from_uv(u, v, 0.0),
                    u,
                    v,
                    dx,
                    dy,
                }]
            }
        }
    };

    let taken = collect_all_doc_ids(doc);
    let base_prefix = match req.id_prefix.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(explicit) => explicit.trim().to_string(),
        None => {
            let root_name = req.kind.id();
            if !taken.contains(root_name)
                && !taken.contains(&format!("{root_name}_bore"))
                && !taken.contains(&format!("{root_name}_head"))
            {
                root_name.to_string()
            } else {
                let mut idx = 1usize;
                loop {
                    let cand = format!("{root_name}_{idx}");
                    if !taken.contains(&cand) && !taken.contains(&format!("{cand}_bore")) {
                        break cand;
                    }
                    idx += 1;
                }
            }
        }
    };

    let is_bolt_or_screw = req.kind.is_bolt_or_screw();
    let has_washer = req.include_washer && is_bolt_or_screw;
    let nut_kind = if is_bolt_or_screw {
        req.associated_nut
    } else {
        AssociatedNutKind::None
    };

    let env_input = FixingEnvelopeInput {
        fixing_kind: req.kind,
        nominal_d_mm: size_spec.major_d,
        pitch_mm: size_spec.pitch,
        engagement_length_mm: req.engagement_mm.max(2.0 * size_spec.pitch),
        boss_od_mm: size_spec.boss_od,
        substrate_material: sub_mat_id.clone(),
        fastener_material: req.fastener_material.clone(),
        vibration_g: req.vibration_g,
        shock_g: req.vibration_g * 2.5,
        payload_mass_kg: req.payload_mass_kg,
        num_fixings: placements.len().max(1),
        delta_temp_c: 0.0,
        service_hours: 1000.0,
        target_failure_prob: 1.0e-3,
        mu_thread: if regime == SubstrateRegime::PlasticBoss {
            0.18
        } else {
            0.14
        },
        mu_head: if regime == SubstrateRegime::PlasticBoss {
            0.20
        } else {
            0.15
        },
        mu_joint: 0.25,
        include_washer: has_washer,
        associated_nut: nut_kind,
        weibull_m_override: None,
        weibull_sigma0_override_mpa: None,
    };
    let envelope = evaluate_fixing_torque_envelope_with_doc(Some(doc), &env_input);

    let mut host_features_added = Vec::new();
    let mut hardware_nodes_added = Vec::new();
    let mut regions_added = Vec::new();
    let mut loads_added = Vec::new();
    let mut lets_added = Vec::new();

    let pilot_d = match regime {
        SubstrateRegime::PlasticBoss => size_spec.pilot_d_plastic,
        _ => size_spec.pilot_d_metal,
    };
    let pilot_r = round3(pilot_d * 0.5);
    let major_r = size_spec.major_d * 0.5;
    let clear_r = size_spec.clearance_d * 0.5;
    let eng_h = req.engagement_mm.max(3.0);
    let washer_shift_z = if has_washer { size_spec.washer_t } else { 0.0 };

    let let_d = format!("{base_prefix}_d");
    let let_pitch = format!("{base_prefix}_pitch");
    let let_pilot_r = format!("{base_prefix}_pilot_r");
    let let_torque = format!("{base_prefix}_torque_nm");
    let let_preload = format!("{base_prefix}_preload_n");
    let let_f_sep = format!("{base_prefix}_f_sep_n");
    let let_f_slip = format!("{base_prefix}_f_slip_n");
    let preload_round = (envelope.recommended_preload_n * 10.0).round() / 10.0;

    upsert_let(doc, &let_d, size_spec.major_d);
    upsert_let(doc, &let_pitch, size_spec.pitch);
    upsert_let(doc, &let_pilot_r, pilot_r);
    upsert_let(doc, &let_torque, round3(envelope.recommended_torque_nm));
    upsert_let(doc, &let_preload, preload_round);
    upsert_let(
        doc,
        &let_f_sep,
        envelope
            .holdable_forces
            .max_axial_load_before_gapping_n
            .round(),
    );
    upsert_let(
        doc,
        &let_f_slip,
        envelope
            .holdable_forces
            .max_transverse_friction_slip_n
            .round(),
    );
    lets_added.extend([
        let_d,
        let_pitch,
        let_pilot_r.clone(),
        let_torque,
        let_preload.clone(),
        let_f_sep,
        let_f_slip,
    ]);

    if let Some(cv) = req.preload_cv.filter(|v| v.is_finite() && *v > 0.0) {
        let label = format!("{base_prefix} preload scatter");
        if let Some(slot) = doc
            .uncertainties
            .iter_mut()
            .find(|u| u.param == let_preload)
        {
            slot.cv = Some(cv);
            slot.unit = "N".into();
        } else {
            doc.uncertainties.push(UncertaintySpec {
                label,
                param: let_preload.clone(),
                std: None,
                cv: Some(cv),
                unit: "N".into(),
            });
        }
    }

    let preload_case = req
        .load_case
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("default");
    let use_group = req.group_in_tree.unwrap_or(true);
    let bind_lets = req.bind_surface_lets.unwrap_or(req.at.is_none()) && placements.len() == 1;
    let rot = if req.at.is_some() && req.host_face.is_none() {
        [0.0, 0.0, 0.0]
    } else {
        frame.rotate_deg
    };

    let mut created_nodes = Vec::new();
    let mut created_group_ids = Vec::new();
    let mut first_new_id = String::new();
    let mut first_bore_group_id = String::new();

    for (idx, pl) in placements.iter().enumerate() {
        let inst_prefix = if placements.len() == 1 {
            base_prefix.clone()
        } else {
            format!("{base_prefix}_{}", idx + 1)
        };
        let [lx, ly, lz] = if use_group {
            [0.0, 0.0, 0.0]
        } else {
            pl.surface_pt
        };
        let child_rot = if use_group { [0.0, 0.0, 0.0] } else { rot };
        let bore_child_op = if use_group { Op::Add } else { Op::Subtract };

        let mut bore_kids = Vec::new();
        let mut hw_kids = Vec::new();

        match req.kind {
            FixingKind::SelfTappingScrew => {
                if regime == SubstrateRegime::PlasticBoss {
                    let boss_id = format!("{inst_prefix}_boss");
                    hw_kids.push(Node {
                        id: boss_id.clone(),
                        op: Op::Add,
                        kind: NodeKind::Cylinder {
                            radius: size_spec.boss_od * 0.5,
                            height: eng_h,
                            round: 0.3,
                        },
                        transform: Transform {
                            translate: [lx, ly, lz - eng_h * 0.5],
                            rotate: child_rot,
                        },
                        blend: if use_group { 0.0 } else { 0.4 },
                        suppressed: false,
                        material: None,
                        color: None,
                    });
                    host_features_added.push(boss_id);
                }

                let seat_cbore_id = format!("{inst_prefix}_seat_recess");
                bore_kids.push(Node {
                    id: seat_cbore_id.clone(),
                    op: bore_child_op,
                    kind: NodeKind::Cylinder {
                        radius: size_spec.washer_od * 0.5,
                        height: 0.8,
                        round: 0.0,
                    },
                    transform: Transform {
                        translate: [lx, ly, lz - 0.3],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: None,
                    color: None,
                });
                host_features_added.push(seat_cbore_id.clone());

                let tap_bore_id = format!("{inst_prefix}_tap_bore");
                let bore_r = if nut_kind != AssociatedNutKind::None {
                    clear_r
                } else {
                    pilot_r
                };
                bore_kids.push(Node {
                    id: tap_bore_id.clone(),
                    op: bore_child_op,
                    kind: NodeKind::Cylinder {
                        radius: bore_r,
                        height: eng_h + 1.0,
                        round: 0.0,
                    },
                    transform: Transform {
                        translate: [lx, ly, lz - eng_h * 0.5],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: None,
                    color: None,
                });
                if nut_kind == AssociatedNutKind::None {
                    upsert_consumer(doc, &tap_bore_id, "radius", None, let_pilot_r.clone());
                }
                host_features_added.push(tap_bore_id.clone());

                let thread_bite_id = format!("{inst_prefix}_thread_bite");
                bore_kids.push(Node {
                    id: thread_bite_id.clone(),
                    op: bore_child_op,
                    kind: NodeKind::Torus {
                        major: round3((pilot_r + major_r) * 0.5),
                        minor: round3((major_r - pilot_r).max(0.22)),
                    },
                    transform: Transform {
                        translate: [lx, ly, lz - eng_h * 0.45],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: None,
                    color: None,
                });
                host_features_added.push(thread_bite_id);

                if has_washer {
                    let washer_id = format!("{inst_prefix}_washer");
                    hw_kids.push(Node {
                        id: washer_id.clone(),
                        op: Op::Add,
                        kind: NodeKind::Cylinder {
                            radius: size_spec.washer_od * 0.5,
                            height: size_spec.washer_t,
                            round: 0.1,
                        },
                        transform: Transform {
                            translate: [lx, ly, lz + size_spec.washer_t * 0.5 - 0.05],
                            rotate: child_rot,
                        },
                        blend: 0.0,
                        suppressed: false,
                        material: Some(req.fastener_material.clone()),
                        color: Some("#E2E8F0".into()),
                    });
                    hardware_nodes_added.push(washer_id);
                }

                let head_id = format!("{inst_prefix}_head");
                hw_kids.push(Node {
                    id: head_id.clone(),
                    op: Op::Add,
                    kind: NodeKind::Cylinder {
                        radius: size_spec.cap_head_d * 0.5,
                        height: size_spec.head_h,
                        round: 0.35,
                    },
                    transform: Transform {
                        translate: [lx, ly, lz + washer_shift_z + size_spec.head_h * 0.5 - 0.1],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: Some(req.fastener_material.clone()),
                    color: Some("#94A3B8".into()),
                });
                hardware_nodes_added.push(head_id.clone());
                add_clamp_region_and_preload(
                    doc,
                    &inst_prefix,
                    &seat_cbore_id,
                    &tap_bore_id,
                    &head_id,
                    preload_round,
                    &let_preload,
                    preload_case,
                    &mut regions_added,
                    &mut loads_added,
                );
            }

            FixingKind::HexHeadScrew => {
                let seat_id = format!("{inst_prefix}_seat_spotface");
                bore_kids.push(Node {
                    id: seat_id.clone(),
                    op: bore_child_op,
                    kind: NodeKind::Cylinder {
                        radius: size_spec.washer_od * 0.5,
                        height: 0.6,
                        round: 0.0,
                    },
                    transform: Transform {
                        translate: [lx, ly, lz - 0.2],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: None,
                    color: None,
                });
                host_features_added.push(seat_id.clone());

                let tap_bore_id = format!("{inst_prefix}_tapped_bore");
                let bore_r = if nut_kind != AssociatedNutKind::None {
                    clear_r
                } else {
                    pilot_r
                };
                bore_kids.push(Node {
                    id: tap_bore_id.clone(),
                    op: bore_child_op,
                    kind: NodeKind::Cylinder {
                        radius: bore_r,
                        height: eng_h + 1.0,
                        round: 0.0,
                    },
                    transform: Transform {
                        translate: [lx, ly, lz - eng_h * 0.5],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: None,
                    color: None,
                });
                if nut_kind == AssociatedNutKind::None {
                    upsert_consumer(doc, &tap_bore_id, "radius", None, let_pilot_r.clone());
                }
                host_features_added.push(tap_bore_id.clone());

                if has_washer {
                    let washer_id = format!("{inst_prefix}_washer");
                    hw_kids.push(Node {
                        id: washer_id.clone(),
                        op: Op::Add,
                        kind: NodeKind::Cylinder {
                            radius: size_spec.washer_od * 0.5,
                            height: size_spec.washer_t,
                            round: 0.1,
                        },
                        transform: Transform {
                            translate: [lx, ly, lz + size_spec.washer_t * 0.5 - 0.05],
                            rotate: child_rot,
                        },
                        blend: 0.0,
                        suppressed: false,
                        material: Some(req.fastener_material.clone()),
                        color: Some("#E2E8F0".into()),
                    });
                    hardware_nodes_added.push(washer_id);
                }

                let hex_head_id = format!("{inst_prefix}_hex_head");
                hw_kids.push(Node {
                    id: hex_head_id.clone(),
                    op: Op::Add,
                    kind: NodeKind::Extrude {
                        profile: hex_profile(size_spec.hex_flats_s),
                        height: size_spec.head_h,
                    },
                    transform: Transform {
                        translate: [lx, ly, lz + washer_shift_z + size_spec.head_h * 0.5 - 0.1],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: Some(req.fastener_material.clone()),
                    color: Some("#CBD5E1".into()),
                });
                hardware_nodes_added.push(hex_head_id.clone());
                add_clamp_region_and_preload(
                    doc,
                    &inst_prefix,
                    &seat_id,
                    &tap_bore_id,
                    &hex_head_id,
                    preload_round,
                    &let_preload,
                    preload_case,
                    &mut regions_added,
                    &mut loads_added,
                );
            }

            FixingKind::HexCapBolt => {
                let cbore_id = format!("{inst_prefix}_counterbore");
                // Keep a shallow 0.4 mm spotface seat whether or not an underhead washer is used
                // so the cylindrical socket cap head sits cleanly proud above the host surface.
                let cbore_depth = 0.4;
                bore_kids.push(Node {
                    id: cbore_id.clone(),
                    op: bore_child_op,
                    kind: NodeKind::Cylinder {
                        radius: (if has_washer {
                            size_spec.washer_od
                        } else {
                            size_spec.cap_head_d
                        }) * 0.5
                            + 0.3,
                        height: cbore_depth,
                        round: 0.0,
                    },
                    transform: Transform {
                        translate: [lx, ly, lz - cbore_depth * 0.5 + 0.1],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: None,
                    color: None,
                });
                host_features_added.push(cbore_id.clone());

                let tap_bore_id = format!("{inst_prefix}_tap_bore");
                let bore_r = if nut_kind != AssociatedNutKind::None {
                    clear_r
                } else {
                    pilot_r
                };
                bore_kids.push(Node {
                    id: tap_bore_id.clone(),
                    op: bore_child_op,
                    kind: NodeKind::Cylinder {
                        radius: bore_r,
                        height: eng_h + 1.0,
                        round: 0.0,
                    },
                    transform: Transform {
                        translate: [lx, ly, lz - eng_h * 0.5],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: None,
                    color: None,
                });
                if nut_kind == AssociatedNutKind::None {
                    upsert_consumer(doc, &tap_bore_id, "radius", None, let_pilot_r.clone());
                }
                host_features_added.push(tap_bore_id.clone());

                if has_washer {
                    let washer_id = format!("{inst_prefix}_washer");
                    hw_kids.push(Node {
                        id: washer_id.clone(),
                        op: Op::Add,
                        kind: NodeKind::Cylinder {
                            radius: size_spec.washer_od * 0.5,
                            height: size_spec.washer_t,
                            round: 0.1,
                        },
                        transform: Transform {
                            translate: [
                                lx,
                                ly,
                                lz - (cbore_depth - 0.1) + size_spec.washer_t * 0.5,
                            ],
                            rotate: child_rot,
                        },
                        blend: 0.0,
                        suppressed: false,
                        material: Some(req.fastener_material.clone()),
                        color: Some("#E2E8F0".into()),
                    });
                    hardware_nodes_added.push(washer_id);
                }

                let cap_head_id = format!("{inst_prefix}_cap_head");
                let head_z = lz - (cbore_depth - 0.1) + washer_shift_z + size_spec.head_h * 0.5;
                hw_kids.push(Node {
                    id: cap_head_id.clone(),
                    op: Op::Add,
                    kind: NodeKind::Cylinder {
                        radius: size_spec.cap_head_d * 0.5,
                        height: size_spec.head_h,
                        round: 0.25,
                    },
                    transform: Transform {
                        translate: [lx, ly, head_z],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: Some(req.fastener_material.clone()),
                    color: Some("#64748B".into()),
                });
                hardware_nodes_added.push(cap_head_id.clone());

                let allen_socket_id = format!("{inst_prefix}_allen_socket");
                hw_kids.push(Node {
                    id: allen_socket_id.clone(),
                    op: Op::Subtract,
                    kind: NodeKind::Extrude {
                        profile: hex_profile(size_spec.allen_s),
                        height: size_spec.head_h * 0.6,
                    },
                    transform: Transform {
                        translate: [lx, ly, head_z + size_spec.head_h * 0.25],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: None,
                    color: None,
                });
                hardware_nodes_added.push(allen_socket_id);

                add_clamp_region_and_preload(
                    doc,
                    &inst_prefix,
                    &cbore_id,
                    &tap_bore_id,
                    &cap_head_id,
                    preload_round,
                    &let_preload,
                    preload_case,
                    &mut regions_added,
                    &mut loads_added,
                );
            }

            FixingKind::Washer => {
                let spotface_id = format!("{inst_prefix}_spotface");
                bore_kids.push(Node {
                    id: spotface_id.clone(),
                    op: bore_child_op,
                    kind: NodeKind::Cylinder {
                        radius: size_spec.washer_od * 0.5 + 0.2,
                        height: 0.5,
                        round: 0.0,
                    },
                    transform: Transform {
                        translate: [lx, ly, lz - 0.15],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: None,
                    color: None,
                });
                host_features_added.push(spotface_id.clone());

                let clear_bore_id = format!("{inst_prefix}_clearance_bore");
                bore_kids.push(Node {
                    id: clear_bore_id.clone(),
                    op: bore_child_op,
                    kind: NodeKind::Cylinder {
                        radius: clear_r,
                        height: eng_h + 1.0,
                        round: 0.0,
                    },
                    transform: Transform {
                        translate: [lx, ly, lz - eng_h * 0.5],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: None,
                    color: None,
                });
                host_features_added.push(clear_bore_id.clone());

                let washer_disc_id = format!("{inst_prefix}_disc");
                let wz = lz + size_spec.washer_t * 0.5 - 0.05;
                hw_kids.push(Node {
                    id: washer_disc_id.clone(),
                    op: Op::Add,
                    kind: NodeKind::Cylinder {
                        radius: size_spec.washer_od * 0.5,
                        height: size_spec.washer_t,
                        round: 0.1,
                    },
                    transform: Transform {
                        translate: [lx, ly, wz],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: Some(req.fastener_material.clone()),
                    color: Some("#94A3B8".into()),
                });
                hardware_nodes_added.push(washer_disc_id.clone());

                let washer_id_hole = format!("{inst_prefix}_id_bore");
                hw_kids.push(Node {
                    id: washer_id_hole.clone(),
                    op: Op::Subtract,
                    kind: NodeKind::Cylinder {
                        radius: clear_r,
                        height: size_spec.washer_t + 0.4,
                        round: 0.0,
                    },
                    transform: Transform {
                        translate: [lx, ly, wz],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: None,
                    color: None,
                });
                hardware_nodes_added.push(washer_id_hole);

                add_clamp_region_and_preload(
                    doc,
                    &inst_prefix,
                    &spotface_id,
                    &clear_bore_id,
                    &washer_disc_id,
                    preload_round,
                    &let_preload,
                    preload_case,
                    &mut regions_added,
                    &mut loads_added,
                );
            }

            FixingKind::HexNut | FixingKind::NylocNut => {
                let nut_pocket_id = format!("{inst_prefix}_nut_pocket");
                let pocket_depth = (size_spec.nut_h * 0.65).max(1.4);
                bore_kids.push(Node {
                    id: nut_pocket_id.clone(),
                    op: bore_child_op,
                    kind: NodeKind::Extrude {
                        profile: hex_profile(size_spec.hex_flats_s + 0.3),
                        height: pocket_depth,
                    },
                    transform: Transform {
                        translate: [lx, ly, lz - pocket_depth * 0.5 + 0.1],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: None,
                    color: None,
                });
                host_features_added.push(nut_pocket_id.clone());

                let bolt_bore_id = format!("{inst_prefix}_bolt_bore");
                bore_kids.push(Node {
                    id: bolt_bore_id.clone(),
                    op: bore_child_op,
                    kind: NodeKind::Cylinder {
                        radius: clear_r,
                        height: eng_h + 1.0,
                        round: 0.0,
                    },
                    transform: Transform {
                        translate: [lx, ly, lz - eng_h * 0.5],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: None,
                    color: None,
                });
                host_features_added.push(bolt_bore_id.clone());

                let nut_body_id = format!("{inst_prefix}_hex_body");
                let nz = lz - (pocket_depth - 0.1) + size_spec.nut_h * 0.5;
                hw_kids.push(Node {
                    id: nut_body_id.clone(),
                    op: Op::Add,
                    kind: NodeKind::Extrude {
                        profile: hex_profile(size_spec.hex_flats_s),
                        height: size_spec.nut_h,
                    },
                    transform: Transform {
                        translate: [lx, ly, nz],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: Some(req.fastener_material.clone()),
                    color: Some("#CBD5E1".into()),
                });
                hardware_nodes_added.push(nut_body_id.clone());

                let mut extra_h = 0.0;
                if req.kind == FixingKind::NylocNut {
                    let nylon_h = (size_spec.nut_h * 0.42).max(1.2);
                    extra_h = nylon_h;
                    let nylon_ring_id = format!("{inst_prefix}_nylon_insert");
                    hw_kids.push(Node {
                        id: nylon_ring_id.clone(),
                        op: Op::Add,
                        kind: NodeKind::Cylinder {
                            radius: size_spec.hex_flats_s * 0.46,
                            height: nylon_h,
                            round: 0.35,
                        },
                        transform: Transform {
                            translate: [lx, ly, nz + size_spec.nut_h * 0.5 + nylon_h * 0.45],
                            rotate: child_rot,
                        },
                        blend: 0.0,
                        suppressed: false,
                        material: Some("pa12".into()),
                        color: Some("#38BDF8".into()),
                    });
                    hardware_nodes_added.push(nylon_ring_id);
                }

                let nut_tap_id = format!("{inst_prefix}_internal_thread");
                hw_kids.push(Node {
                    id: nut_tap_id.clone(),
                    op: Op::Subtract,
                    kind: NodeKind::Cylinder {
                        radius: pilot_r,
                        height: size_spec.nut_h + extra_h + 1.0,
                        round: 0.0,
                    },
                    transform: Transform {
                        translate: [lx, ly, nz + extra_h * 0.5],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: None,
                    color: None,
                });
                hardware_nodes_added.push(nut_tap_id);

                add_clamp_region_and_preload(
                    doc,
                    &inst_prefix,
                    &nut_pocket_id,
                    &bolt_bore_id,
                    &nut_body_id,
                    preload_round,
                    &let_preload,
                    preload_case,
                    &mut regions_added,
                    &mut loads_added,
                );
            }

            FixingKind::HookClipBox => {
                let beam_w = (size_spec.major_d * 1.6).max(5.0);
                let beam_t = (size_spec.major_d * 0.45).max(1.6);
                let beam_l = req.engagement_mm.max(10.0);
                let undercut_d = (beam_t * 0.55).max(0.9);

                let catch_slot_id = format!("{inst_prefix}_catch_slot");
                bore_kids.push(Node {
                    id: catch_slot_id.clone(),
                    op: bore_child_op,
                    kind: NodeKind::Box {
                        size: [beam_t + undercut_d + 0.5, beam_w + 0.8, 3.2],
                        round: 0.2,
                    },
                    transform: Transform {
                        translate: [lx, ly, lz - 2.2],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: None,
                    color: None,
                });
                host_features_added.push(catch_slot_id.clone());

                let arm_id = format!("{inst_prefix}_cantilever_arm");
                hw_kids.push(Node {
                    id: arm_id.clone(),
                    op: Op::Add,
                    kind: NodeKind::Box {
                        size: [beam_t, beam_w, beam_l],
                        round: 0.35,
                    },
                    transform: Transform {
                        translate: [lx + beam_t * 0.5 + 0.25, ly, lz + beam_l * 0.5 - 1.5],
                        rotate: child_rot,
                    },
                    blend: if use_group { 0.0 } else { 0.6 },
                    suppressed: false,
                    material: None,
                    color: Some("#38BDF8".into()),
                });
                hardware_nodes_added.push(arm_id);

                let hook_tip_id = format!("{inst_prefix}_hook_tip");
                hw_kids.push(Node {
                    id: hook_tip_id.clone(),
                    op: Op::Add,
                    kind: NodeKind::Box {
                        size: [beam_t + undercut_d, beam_w, 2.6],
                        round: 0.3,
                    },
                    transform: Transform {
                        translate: [
                            lx + beam_t * 0.5 + 0.25 - undercut_d * 0.5,
                            ly,
                            lz + beam_l - 2.5,
                        ],
                        rotate: child_rot,
                    },
                    blend: 0.25,
                    suppressed: false,
                    material: None,
                    color: Some("#0284C7".into()),
                });
                hardware_nodes_added.push(hook_tip_id.clone());

                let reg_id = format!("{inst_prefix}_snap_ledge");
                doc.regions.push(Region {
                    id: reg_id.clone(),
                    selectors: vec![
                        Selector {
                            shape: hook_tip_id,
                            face: Face::Nx,
                            patch: None,
                        },
                        Selector {
                            shape: catch_slot_id,
                            face: Face::Nz,
                            patch: None,
                        },
                    ],
                });
                regions_added.push(reg_id.clone());

                let load_id = format!("{inst_prefix}_snap_load");
                doc.loads.push(Load {
                    id: load_id.clone(),
                    region: reg_id,
                    case: "default".into(),
                    kind: LoadKind::Force {
                        vector: [-envelope.recommended_preload_n.min(45.0).round(), 0.0, 0.0],
                    },
                });
                loads_added.push(load_id);
            }
        }

        // Append associated nut (`hex_nut` or `nyloc_nut`) on bolt/screw stacks when requested.
        if is_bolt_or_screw && nut_kind != AssociatedNutKind::None {
            let pocket_depth = (size_spec.nut_h * 0.65).max(1.4);
            let nut_z = lz - eng_h + size_spec.nut_h * 0.5;
            let nut_pocket_id = format!("{inst_prefix}_nut_pocket");
            bore_kids.push(Node {
                id: nut_pocket_id.clone(),
                op: bore_child_op,
                kind: NodeKind::Extrude {
                    profile: hex_profile(size_spec.hex_flats_s + 0.35),
                    height: pocket_depth + 0.4,
                },
                transform: Transform {
                    translate: [lx, ly, lz - eng_h + pocket_depth * 0.5 - 0.1],
                    rotate: child_rot,
                },
                blend: 0.0,
                suppressed: false,
                material: None,
                color: None,
            });
            host_features_added.push(nut_pocket_id);

            let shank_id = format!("{inst_prefix}_shank");
            hw_kids.push(Node {
                id: shank_id.clone(),
                op: Op::Add,
                kind: NodeKind::Cylinder {
                    radius: round3(major_r * 0.92),
                    height: eng_h + size_spec.nut_h * 0.5,
                    round: 0.0,
                },
                transform: Transform {
                    translate: [lx, ly, lz - eng_h * 0.5],
                    rotate: child_rot,
                },
                blend: 0.0,
                suppressed: false,
                material: Some(req.fastener_material.clone()),
                color: Some("#94A3B8".into()),
            });
            hardware_nodes_added.push(shank_id);

            let nut_body_id = format!("{inst_prefix}_nut_body");
            hw_kids.push(Node {
                id: nut_body_id.clone(),
                op: Op::Add,
                kind: NodeKind::Extrude {
                    profile: hex_profile(size_spec.hex_flats_s),
                    height: size_spec.nut_h,
                },
                transform: Transform {
                    translate: [lx, ly, nut_z],
                    rotate: child_rot,
                },
                blend: 0.0,
                suppressed: false,
                material: Some(req.fastener_material.clone()),
                color: Some("#CBD5E1".into()),
            });
            hardware_nodes_added.push(nut_body_id);

            if nut_kind == AssociatedNutKind::NylocNut {
                let nylon_h = (size_spec.nut_h * 0.38).max(1.1);
                let nylon_id = format!("{inst_prefix}_nylon_collar");
                hw_kids.push(Node {
                    id: nylon_id.clone(),
                    op: Op::Add,
                    kind: NodeKind::Cylinder {
                        radius: round3(size_spec.hex_flats_s * 0.46),
                        height: nylon_h,
                        round: 0.3,
                    },
                    transform: Transform {
                        translate: [lx, ly, nut_z - size_spec.nut_h * 0.5 - nylon_h * 0.4],
                        rotate: child_rot,
                    },
                    blend: 0.0,
                    suppressed: false,
                    material: Some("pa12".into()),
                    color: Some("#38BDF8".into()),
                });
                hardware_nodes_added.push(nylon_id);
            }
        }

        if use_group {
            let bore_group_id = format!("{inst_prefix}_bore");
            let hw_group_id = inst_prefix.clone();
            created_nodes.push(Node {
                id: bore_group_id.clone(),
                op: Op::Subtract,
                blend: 0.0,
                suppressed: false,
                transform: Transform {
                    translate: pl.surface_pt,
                    rotate: rot,
                },
                material: None,
                color: None,
                kind: NodeKind::Group {
                    children: bore_kids,
                    modifiers: Vec::new(),
                },
            });
            created_nodes.push(Node {
                id: hw_group_id.clone(),
                op: Op::Add,
                blend: 0.0,
                suppressed: false,
                transform: Transform {
                    translate: pl.surface_pt,
                    rotate: rot,
                },
                material: None,
                color: None,
                kind: NodeKind::Group {
                    children: hw_kids,
                    modifiers: Vec::new(),
                },
            });
            created_group_ids.push(bore_group_id.clone());
            created_group_ids.push(hw_group_id.clone());
            if first_new_id.is_empty() {
                first_new_id = hw_group_id;
            }
            if first_bore_group_id.is_empty() {
                first_bore_group_id = bore_group_id;
            }
        } else {
            if first_new_id.is_empty() {
                if let Some(first_hw) = hw_kids.first() {
                    first_new_id = first_hw.id.clone();
                }
            }
            created_nodes.extend(bore_kids);
            created_nodes.extend(hw_kids);
        }
    }

    associate_nodes_with_host(
        doc,
        target_host_id.as_deref(),
        created_nodes,
        req.associate_with_host_group,
    );

    if bind_lets && use_group && !first_new_id.is_empty() && !first_bore_group_id.is_empty() {
        let pt0 = placements[0];
        if mode == FixingPlacementMode::Center {
            let u_name = format!("{base_prefix}_u");
            let v_name = format!("{base_prefix}_v");
            upsert_let(doc, &u_name, pt0.u);
            upsert_let(doc, &v_name, pt0.v);
            lets_added.push(u_name.clone());
            lets_added.push(v_name.clone());
            let expr_u = format_offset_expr(frame.face_center[frame.u_axis_index], &u_name);
            let expr_v = format_offset_expr(frame.face_center[frame.v_axis_index], &v_name);
            for gid in [&first_bore_group_id, &first_new_id] {
                upsert_consumer(doc, gid, "at", Some(frame.u_axis_index), expr_u.clone());
                upsert_consumer(doc, gid, "at", Some(frame.v_axis_index), expr_v.clone());
            }
        } else {
            let dx_name = format!("{base_prefix}_dx");
            let dy_name = format!("{base_prefix}_dy");
            upsert_let(doc, &dx_name, pt0.dx);
            upsert_let(doc, &dy_name, pt0.dy);
            lets_added.push(dx_name.clone());
            lets_added.push(dy_name.clone());
            let base_u = frame.face_center[frame.u_axis_index] - 0.5 * frame.face_width;
            let base_v = frame.face_center[frame.v_axis_index] - 0.5 * frame.face_height;
            let expr_u = format_offset_expr(base_u, &dx_name);
            let expr_v = format_offset_expr(base_v, &dy_name);
            for gid in [&first_bore_group_id, &first_new_id] {
                upsert_consumer(doc, gid, "at", Some(frame.u_axis_index), expr_u.clone());
                upsert_consumer(doc, gid, "at", Some(frame.v_axis_index), expr_v.clone());
            }
        }
    }

    let _ = param::apply(
        doc,
        &param::ParamAddr {
            id: let_preload.clone(),
            attr: String::new(),
            comp: None,
        },
        preload_round,
    );

    FixingInsertSummary {
        kind: req.kind,
        size: size_spec.name,
        id_prefix: base_prefix,
        first_new_id,
        bore_group_id: first_bore_group_id,
        created_group_ids,
        host_features_added,
        hardware_nodes_added,
        regions_added,
        loads_added,
        lets_added,
        envelope,
    }
}

#[allow(clippy::too_many_arguments)]
fn add_clamp_region_and_preload(
    doc: &mut Document,
    inst_prefix: &str,
    seat_shape_id: &str,
    bore_shape_id: &str,
    hardware_head_id: &str,
    preload_n: f64,
    let_preload_name: &str,
    preload_case: &str,
    regions_added: &mut Vec<String>,
    loads_added: &mut Vec<String>,
) {
    let seat_reg = format!("{inst_prefix}_seat");
    doc.regions.push(Region {
        id: seat_reg.clone(),
        selectors: vec![
            Selector {
                shape: hardware_head_id.into(),
                face: Face::Top,
                patch: None,
            },
            Selector {
                shape: seat_shape_id.into(),
                face: Face::Bottom,
                patch: None,
            },
            Selector {
                shape: bore_shape_id.into(),
                face: Face::Side,
                patch: None,
            },
        ],
    });
    regions_added.push(seat_reg.clone());

    let load_id = format!("{inst_prefix}_preload");
    doc.loads.push(Load {
        id: load_id.clone(),
        region: seat_reg,
        case: preload_case.into(),
        kind: LoadKind::Preload {
            newtons: preload_n,
            axis: [0.0, 0.0, 0.0],
        },
    });
    upsert_consumer(doc, &load_id, "newtons", None, let_preload_name.to_string());
    loads_added.push(load_id);
}

// ============================================================================
// Live Inspection & In-Place Reconfiguration of Existing Bolt/Screw Stacks
// ============================================================================

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FixingStackInspection {
    /// Hardware group ID (`<prefix>`, e.g. `hex_cap_bolt` or `hex_cap_bolt_1`).
    pub hw_group_id: String,
    /// Companion bore group ID (`<prefix>_bore`) if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bore_group_id: Option<String>,
    /// Base prefix (`hex_cap_bolt`, `self_tapping_screw_2`, etc.).
    pub base_prefix: String,
    /// Fixing kind (`hex_cap_bolt`, `hex_head_screw`, `self_tapping_screw`).
    pub fixing_kind: FixingKind,
    /// Resolved metric size specification (`M2.5`..`M8`).
    pub size_spec: FixingSizeSpec,
    /// Thread engagement / bolt grip length in mm.
    pub engagement_mm: f64,
    /// Fastener hardware material ID.
    pub fastener_material: String,
    /// Host substrate material ID.
    pub substrate_material: String,
    /// Whether an underhead bearing washer (`<prefix>_washer`) is currently present.
    pub include_washer: bool,
    /// Currently associated mated nut (`none`, `hex_nut`, `nyloc_nut`).
    pub associated_nut: AssociatedNutKind,
    /// Full live torque & holdable-forces report for the current stack.
    pub envelope: FixingEnvelopeReport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FixingReconfigureSpec {
    #[serde(alias = "selectedNodeId")]
    pub selected_node_id: String,
    #[serde(default, alias = "includeWasher")]
    pub include_washer: bool,
    #[serde(default, alias = "associatedNut")]
    pub associated_nut: AssociatedNutKind,
    #[serde(default, alias = "fastenerMaterial")]
    pub fastener_material: Option<String>,
}

fn find_parent_group_id(nodes: &[Node], child_id: &str) -> Option<String> {
    for n in nodes {
        if let NodeKind::Group { children, .. } = &n.kind {
            if children.iter().any(|c| c.id == child_id) {
                return Some(n.id.clone());
            }
            if let Some(sub) = find_parent_group_id(children, child_id) {
                return Some(sub);
            }
        }
    }
    None
}

fn find_node_mut<'a>(nodes: &'a mut [Node], id: &str) -> Option<&'a mut Node> {
    for n in nodes {
        if n.id == id {
            return Some(n);
        }
        if let NodeKind::Group { children, .. } = &mut n.kind {
            if let Some(sub) = find_node_mut(children, id) {
                return Some(sub);
            }
        }
    }
    None
}

fn strip_numeric_instance_suffix(id: &str, kind: FixingKind) -> &str {
    let root = kind.id();
    if id == root {
        return id;
    }
    if let Some((stem, tail)) = id.rsplit_once('_') {
        if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) && stem.starts_with(root) {
            return stem;
        }
    }
    id
}

fn detect_bolt_fixing_kind(prefix: &str, hw_group: &Node) -> Option<FixingKind> {
    if prefix.starts_with("hex_cap_bolt") {
        return Some(FixingKind::HexCapBolt);
    }
    if prefix.starts_with("hex_head_screw") {
        return Some(FixingKind::HexHeadScrew);
    }
    if prefix.starts_with("self_tapping_screw") {
        return Some(FixingKind::SelfTappingScrew);
    }
    if let NodeKind::Group { children, .. } = &hw_group.kind {
        if children
            .iter()
            .any(|c| c.id.ends_with("_cap_head") || c.id.ends_with("_allen_socket"))
        {
            return Some(FixingKind::HexCapBolt);
        }
        if children.iter().any(|c| c.id.ends_with("_hex_head")) {
            return Some(FixingKind::HexHeadScrew);
        }
        if children
            .iter()
            .any(|c| c.id.ends_with("_head") || c.id.ends_with("_boss"))
        {
            return Some(FixingKind::SelfTappingScrew);
        }
    }
    None
}

/// Inspect an existing grouped bolt/screw stack in `doc` by group or child node ID.
pub fn inspect_fixing_group_stack(
    doc: &Document,
    selected_node_id: &str,
) -> Option<FixingStackInspection> {
    let mut candidate_id = selected_node_id
        .strip_suffix("_bore")
        .unwrap_or(selected_node_id)
        .to_string();
    let mut node = doc.find(&candidate_id);
    if !node.is_some_and(|n| matches!(n.kind, NodeKind::Group { .. })) {
        if let Some(parent_id) = find_parent_group_id(&doc.root, selected_node_id) {
            let clean = parent_id
                .strip_suffix("_bore")
                .unwrap_or(&parent_id)
                .to_string();
            node = doc.find(&clean);
            candidate_id = clean;
        }
    }
    let hw_node = node?;
    let NodeKind::Group { children: kids, .. } = &hw_node.kind else {
        return None;
    };

    let fixing_kind = detect_bolt_fixing_kind(&candidate_id, hw_node)?;
    let stripped = strip_numeric_instance_suffix(&candidate_id, fixing_kind);
    let base_prefix = if doc
        .lets
        .iter()
        .any(|l| l.name == format!("{candidate_id}_d"))
    {
        candidate_id.clone()
    } else if doc.lets.iter().any(|l| l.name == format!("{stripped}_d")) {
        stripped.to_string()
    } else {
        candidate_id.clone()
    };

    let bore_id = format!("{candidate_id}_bore");
    let bore_node = doc.find(&bore_id);
    let bore_kids: &[Node] = match bore_node.map(|n| &n.kind) {
        Some(NodeKind::Group { children, .. }) => children.as_slice(),
        _ => &[],
    };

    let d_let = doc
        .lets
        .iter()
        .find(|l| l.name == format!("{candidate_id}_d") || l.name == format!("{base_prefix}_d"))
        .map(|l| l.value);
    let mut size_spec = find_fixing_size("M4");
    if let Some(d_val) = d_let {
        if let Some(m) = fixing_sizes()
            .into_iter()
            .find(|s| (s.major_d - d_val).abs() < 0.15)
        {
            size_spec = m;
        }
    } else if let Some(head_child) = kids
        .iter()
        .find(|c| c.id.ends_with("_cap_head") || c.id.ends_with("_head"))
    {
        if let NodeKind::Cylinder { radius, .. } = &head_child.kind {
            let cap_d = radius * 2.0;
            if let Some(m) = fixing_sizes()
                .into_iter()
                .find(|s| (s.cap_head_d - cap_d).abs() < 0.35)
            {
                size_spec = m;
            }
        }
    }

    let mut engagement_mm = ((size_spec.major_d * 2.2) * 10.0).round() / 10.0;
    if let Some(tap_bore) = bore_kids
        .iter()
        .find(|c| c.id.ends_with("_tap_bore") || c.id.ends_with("_tapped_bore"))
    {
        if let NodeKind::Cylinder { height, .. } = &tap_bore.kind {
            engagement_mm = (((height - 1.0) * 10.0).round() / 10.0).max(3.0);
        }
    }

    let include_washer = kids
        .iter()
        .any(|c| c.id == format!("{candidate_id}_washer") && !c.suppressed);
    let has_nyloc = kids
        .iter()
        .any(|c| c.id == format!("{candidate_id}_nylon_collar") && !c.suppressed);
    let has_nut_body = kids
        .iter()
        .any(|c| c.id == format!("{candidate_id}_nut_body") && !c.suppressed);
    let associated_nut = if has_nyloc {
        AssociatedNutKind::NylocNut
    } else if has_nut_body {
        AssociatedNutKind::HexNut
    } else {
        AssociatedNutKind::None
    };

    let fastener_material = kids
        .iter()
        .find(|c| {
            c.id.ends_with("_cap_head") || c.id.ends_with("_hex_head") || c.id.ends_with("_head")
        })
        .and_then(|c| c.material.clone())
        .unwrap_or_else(|| "ti64".into());
    let substrate_material = if doc.material.trim().is_empty() {
        "petg".into()
    } else {
        doc.material.clone()
    };
    let regime = SubstrateRegime::classify_material(&substrate_material);

    let envelope = evaluate_fixing_torque_envelope_with_doc(
        Some(doc),
        &FixingEnvelopeInput {
            fixing_kind,
            nominal_d_mm: size_spec.major_d,
            pitch_mm: size_spec.pitch,
            engagement_length_mm: engagement_mm,
            boss_od_mm: size_spec.boss_od,
            substrate_material: substrate_material.clone(),
            fastener_material: fastener_material.clone(),
            vibration_g: 3.5,
            shock_g: 8.75,
            payload_mass_kg: 0.45,
            num_fixings: 1,
            delta_temp_c: 0.0,
            service_hours: 1000.0,
            target_failure_prob: 1.0e-3,
            mu_thread: if regime == SubstrateRegime::PlasticBoss {
                0.18
            } else {
                0.14
            },
            mu_head: if regime == SubstrateRegime::PlasticBoss {
                0.20
            } else {
                0.15
            },
            mu_joint: 0.25,
            include_washer,
            associated_nut,
            weibull_m_override: None,
            weibull_sigma0_override_mpa: None,
        },
    );

    Some(FixingStackInspection {
        hw_group_id: candidate_id,
        bore_group_id: bore_node.map(|n| n.id.clone()),
        base_prefix,
        fixing_kind,
        size_spec,
        engagement_mm,
        fastener_material,
        substrate_material,
        include_washer,
        associated_nut,
        envelope,
    })
}

/// Reconfigure an existing grouped bolt/screw stack in `doc` (toggling ISO 7089 washer,
/// Hex / Nyloc nut, or fastener material) and synchronize `let` parameters + `LoadKind::Preload`.
pub fn reconfigure_existing_bolt_stack(
    doc: &mut Document,
    spec: &FixingReconfigureSpec,
) -> Option<FixingStackInspection> {
    let info = inspect_fixing_group_stack(doc, &spec.selected_node_id)?;
    let fastener_material = spec
        .fastener_material
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| info.fastener_material.clone());
    ensure_fixing_material_in_doc(doc, &fastener_material);

    let size_spec = info.size_spec.clone();
    let engagement_mm = info.engagement_mm;
    let substrate_material = info.substrate_material.clone();
    let fixing_kind = info.fixing_kind;
    let inst_prefix = info.hw_group_id.clone();
    let base_prefix = info.base_prefix.clone();
    let has_washer = spec.include_washer;
    let nut_kind = spec.associated_nut;
    let washer_shift_z = if has_washer { size_spec.washer_t } else { 0.0 };
    let eng_h = engagement_mm.max(3.0);
    let regime = SubstrateRegime::classify_material(&substrate_material);
    let pilot_d = match regime {
        SubstrateRegime::PlasticBoss => size_spec.pilot_d_plastic,
        _ => size_spec.pilot_d_metal,
    };
    let pilot_r = round3(pilot_d * 0.5);
    let clear_r = size_spec.clearance_d * 0.5;
    let major_r = size_spec.major_d * 0.5;

    // 1. Update bore group children (remove old nut pocket, update counterbore/tap bore, add new nut pocket).
    let mut tap_bore_target_id: Option<String> = None;
    if let Some(bore_id) = &info.bore_group_id {
        if let Some(bore_group) = find_node_mut(&mut doc.root, bore_id) {
            if let NodeKind::Group { children, .. } = &mut bore_group.kind {
                children.retain(|c| c.id != format!("{inst_prefix}_nut_pocket"));
                if fixing_kind == FixingKind::HexCapBolt {
                    let cbore_depth = 0.4;
                    if let Some(cbore) = children
                        .iter_mut()
                        .find(|c| c.id == format!("{inst_prefix}_counterbore"))
                    {
                        if let NodeKind::Cylinder { radius, height, .. } = &mut cbore.kind {
                            *radius = (if has_washer {
                                size_spec.washer_od
                            } else {
                                size_spec.cap_head_d
                            }) * 0.5
                                + 0.3;
                            *height = cbore_depth;
                        }
                        cbore.transform.translate[2] = -cbore_depth * 0.5 + 0.1;
                    }
                }
                if let Some(tap_bore) = children.iter_mut().find(|c| {
                    c.id == format!("{inst_prefix}_tap_bore")
                        || c.id == format!("{inst_prefix}_tapped_bore")
                }) {
                    if let NodeKind::Cylinder { radius, .. } = &mut tap_bore.kind {
                        *radius = if nut_kind != AssociatedNutKind::None {
                            clear_r
                        } else {
                            pilot_r
                        };
                    }
                    tap_bore_target_id = Some(tap_bore.id.clone());
                }
                if nut_kind != AssociatedNutKind::None {
                    let pocket_depth = (size_spec.nut_h * 0.65).max(1.4);
                    children.push(Node {
                        id: format!("{inst_prefix}_nut_pocket"),
                        op: Op::Add,
                        blend: 0.0,
                        suppressed: false,
                        transform: Transform {
                            translate: [0.0, 0.0, -eng_h + pocket_depth * 0.5 - 0.1],
                            rotate: [0.0; 3],
                        },
                        material: None,
                        color: None,
                        kind: NodeKind::Extrude {
                            profile: hex_profile(size_spec.hex_flats_s + 0.35),
                            height: pocket_depth + 0.4,
                        },
                    });
                }
            }
        }
    }

    if let Some(tb_id) = tap_bore_target_id {
        if nut_kind != AssociatedNutKind::None {
            doc.consumed
                .retain(|c| !(c.target == tb_id && c.attr == "radius"));
        } else {
            let let_pilot = format!("{base_prefix}_pilot_r");
            if doc.lets.iter().any(|l| l.name == let_pilot) {
                upsert_consumer(doc, &tb_id, "radius", None, let_pilot);
            }
        }
    }

    // 2. Update hardware group children (material, head elevation, washer, shank, nut, nylon collar).
    if let Some(hw_group) = find_node_mut(&mut doc.root, &inst_prefix) {
        if let NodeKind::Group { children, .. } = &mut hw_group.kind {
            children.retain(|c| {
                c.id != format!("{inst_prefix}_washer")
                    && c.id != format!("{inst_prefix}_shank")
                    && c.id != format!("{inst_prefix}_nut_body")
                    && c.id != format!("{inst_prefix}_nylon_collar")
            });
            for child in children.iter_mut() {
                if child.op == Op::Add
                    && !child.id.ends_with("_nylon_collar")
                    && !child.id.ends_with("_boss")
                {
                    child.material = Some(fastener_material.clone());
                }
            }

            if fixing_kind == FixingKind::HexCapBolt {
                let cbore_depth = 0.4;
                let head_z = -(cbore_depth - 0.1) + washer_shift_z + size_spec.head_h * 0.5;
                if let Some(cap_head) = children
                    .iter_mut()
                    .find(|c| c.id == format!("{inst_prefix}_cap_head"))
                {
                    cap_head.transform.translate[2] = head_z;
                }
                if let Some(allen) = children
                    .iter_mut()
                    .find(|c| c.id == format!("{inst_prefix}_allen_socket"))
                {
                    allen.transform.translate[2] = head_z + size_spec.head_h * 0.25;
                }
                if has_washer {
                    children.insert(
                        0,
                        Node {
                            id: format!("{inst_prefix}_washer"),
                            op: Op::Add,
                            blend: 0.0,
                            suppressed: false,
                            material: Some(fastener_material.clone()),
                            color: Some("#E2E8F0".into()),
                            transform: Transform {
                                translate: [
                                    0.0,
                                    0.0,
                                    -(cbore_depth - 0.1) + size_spec.washer_t * 0.5,
                                ],
                                rotate: [0.0; 3],
                            },
                            kind: NodeKind::Cylinder {
                                radius: size_spec.washer_od * 0.5,
                                height: size_spec.washer_t,
                                round: 0.1,
                            },
                        },
                    );
                }
            } else {
                if let Some(head_node) = children.iter_mut().find(|c| {
                    c.id == format!("{inst_prefix}_head")
                        || c.id == format!("{inst_prefix}_hex_head")
                }) {
                    head_node.transform.translate[2] =
                        washer_shift_z + size_spec.head_h * 0.5 - 0.1;
                }
                if has_washer {
                    children.insert(
                        0,
                        Node {
                            id: format!("{inst_prefix}_washer"),
                            op: Op::Add,
                            blend: 0.0,
                            suppressed: false,
                            material: Some(fastener_material.clone()),
                            color: Some("#E2E8F0".into()),
                            transform: Transform {
                                translate: [0.0, 0.0, size_spec.washer_t * 0.5 - 0.05],
                                rotate: [0.0; 3],
                            },
                            kind: NodeKind::Cylinder {
                                radius: size_spec.washer_od * 0.5,
                                height: size_spec.washer_t,
                                round: 0.1,
                            },
                        },
                    );
                }
            }

            if nut_kind != AssociatedNutKind::None {
                let nut_z = -eng_h + size_spec.nut_h * 0.5;
                children.push(Node {
                    id: format!("{inst_prefix}_shank"),
                    op: Op::Add,
                    blend: 0.0,
                    suppressed: false,
                    material: Some(fastener_material.clone()),
                    color: Some("#94A3B8".into()),
                    transform: Transform {
                        translate: [0.0, 0.0, -eng_h * 0.5],
                        rotate: [0.0; 3],
                    },
                    kind: NodeKind::Cylinder {
                        radius: round3(major_r * 0.92),
                        height: eng_h + size_spec.nut_h * 0.5,
                        round: 0.0,
                    },
                });
                children.push(Node {
                    id: format!("{inst_prefix}_nut_body"),
                    op: Op::Add,
                    blend: 0.0,
                    suppressed: false,
                    material: Some(fastener_material.clone()),
                    color: Some("#CBD5E1".into()),
                    transform: Transform {
                        translate: [0.0, 0.0, nut_z],
                        rotate: [0.0; 3],
                    },
                    kind: NodeKind::Extrude {
                        profile: hex_profile(size_spec.hex_flats_s),
                        height: size_spec.nut_h,
                    },
                });
                if nut_kind == AssociatedNutKind::NylocNut {
                    let nylon_h = (size_spec.nut_h * 0.38).max(1.1);
                    children.push(Node {
                        id: format!("{inst_prefix}_nylon_collar"),
                        op: Op::Add,
                        blend: 0.0,
                        suppressed: false,
                        material: Some("pa12".into()),
                        color: Some("#38BDF8".into()),
                        transform: Transform {
                            translate: [0.0, 0.0, nut_z - size_spec.nut_h * 0.5 - nylon_h * 0.4],
                            rotate: [0.0; 3],
                        },
                        kind: NodeKind::Cylinder {
                            radius: round3(size_spec.hex_flats_s * 0.46),
                            height: nylon_h,
                            round: 0.3,
                        },
                    });
                }
            }
        }
    }

    // 3. Re-inspect updated stack, synchronize `let`s and `LoadKind::Preload` (fixing the stale force-only bug).
    let updated = inspect_fixing_group_stack(doc, &inst_prefix)?;
    let env = &updated.envelope;
    let new_preload = (env.recommended_preload_n * 10.0).round() / 10.0;
    let let_preload_name = format!("{base_prefix}_preload_n");

    upsert_let(
        doc,
        &format!("{base_prefix}_torque_nm"),
        round3(env.recommended_torque_nm),
    );
    upsert_let(doc, &let_preload_name, new_preload);
    upsert_let(
        doc,
        &format!("{base_prefix}_f_sep_n"),
        env.holdable_forces.max_axial_load_before_gapping_n.round(),
    );
    upsert_let(
        doc,
        &format!("{base_prefix}_f_slip_n"),
        env.holdable_forces.max_transverse_friction_slip_n.round(),
    );

    let mut matched_load_ids = Vec::new();
    for ld in &mut doc.loads {
        if ld.id == format!("{inst_prefix}_preload") || ld.id == format!("{base_prefix}_preload") {
            match &mut ld.kind {
                LoadKind::Preload { newtons, .. } => {
                    *newtons = new_preload;
                    matched_load_ids.push(ld.id.clone());
                }
                LoadKind::Force { vector } => {
                    let mag =
                        (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2])
                            .sqrt();
                    if mag > 1.0e-6 {
                        let scale = new_preload.max(5.0) / mag;
                        vector[0] = round3(vector[0] * scale);
                        vector[1] = round3(vector[1] * scale);
                        vector[2] = round3(vector[2] * scale);
                    }
                }
                _ => {}
            }
        }
    }
    for lid in matched_load_ids {
        upsert_consumer(doc, &lid, "newtons", None, let_preload_name.clone());
    }
    let _ = param::apply(
        doc,
        &param::ParamAddr {
            id: let_preload_name,
            attr: String::new(),
            comp: None,
        },
        new_preload,
    );

    inspect_fixing_group_stack(doc, &inst_prefix)
}

// ============================================================================
// Minimal & Maximal Torque Envelope + Weibull / Creep Failure Distribution Fit
// ============================================================================

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FixingEnvelopeInput {
    #[serde(default, alias = "fixingKind")]
    pub fixing_kind: FixingKind,
    /// Nominal screw major diameter `d` (mm).
    #[serde(default = "default_nom_d", alias = "nominalDMm")]
    pub nominal_d_mm: f64,
    /// Thread pitch `P` (mm).
    #[serde(default = "default_pitch", alias = "pitchMm")]
    pub pitch_mm: f64,
    /// Engaged thread length `L_eng` (mm).
    #[serde(default = "default_engagement_mm", alias = "engagementLengthMm")]
    pub engagement_length_mm: f64,
    /// Outer diameter of receiving boss or local housing flange `D_boss` (mm).
    #[serde(default = "default_boss_od", alias = "bossOdMm")]
    pub boss_od_mm: f64,
    /// Substrate material ID (`"petg"`, `"pa12"`, `"alsi10mg"`, `"ti64"`, `"sic_jwst"`, `"cf_woven_3k"`, etc.).
    #[serde(default = "default_substrate_material", alias = "substrateMaterial")]
    pub substrate_material: String,
    /// Fastener hardware material ID (`"ti64"`, `"ss316l"`, `"steel_10_9"`, `"in718"`, etc.).
    #[serde(default = "default_fastener_material", alias = "fastenerMaterial")]
    pub fastener_material: String,
    /// Harmonic / random RMS vibration acceleration in g's (`1 g = 9.80665 m/s²`).
    #[serde(default = "default_vibration_g", alias = "vibrationG")]
    pub vibration_g: f64,
    /// Peak axial knock / launch shock acceleration in g's.
    #[serde(default = "default_shock_g", alias = "shockG")]
    pub shock_g: f64,
    /// Tributary mass supported by the joint (kg).
    #[serde(default = "default_payload_mass_kg", alias = "payloadMassKg")]
    pub payload_mass_kg: f64,
    /// Number of fixings sharing the tributary payload.
    #[serde(default = "default_num_fixings", alias = "numFixings")]
    pub num_fixings: usize,
    /// Operating temperature excursion `ΔT = T_op − T_install` (°C), e.g. `−235 °C` for JWST cryo cooldown.
    #[serde(default, alias = "deltaTempC")]
    pub delta_temp_c: f64,
    /// Mission / service dwell duration (hours) for viscoplastic creep relaxation.
    #[serde(default = "default_service_hours", alias = "serviceHours")]
    pub service_hours: f64,
    /// Target maximum allowable failure probability `P_f,allow` (e.g. `1e-3` industrial, `1e-6` JWST spaceflight).
    #[serde(default = "default_target_pf", alias = "targetFailureProb")]
    pub target_failure_prob: f64,
    /// Thread flank sliding friction coefficient `μ_t`.
    #[serde(default = "default_mu_thread", alias = "muThread")]
    pub mu_thread: f64,
    /// Under-head / washer bearing sliding friction coefficient `μ_h`.
    #[serde(default = "default_mu_head", alias = "muHead")]
    pub mu_head: f64,
    /// Clamped joint interface static friction coefficient `μ_joint`.
    #[serde(default = "default_mu_joint", alias = "muJoint")]
    pub mu_joint: f64,
    /// Whether an ISO 7089 / DIN 125A bearing washer is installed under the head.
    #[serde(default, alias = "includeWasher")]
    pub include_washer: bool,
    /// Optional mated nut associated with the bolt (`none`, `hex_nut`, `nyloc_nut`).
    #[serde(default, alias = "associatedNut")]
    pub associated_nut: AssociatedNutKind,
    /// Optional empirical Weibull modulus `m` override from spreadsheet regression.
    #[serde(default, alias = "weibullMOverride")]
    pub weibull_m_override: Option<f64>,
    /// Optional empirical Weibull scale parameter `σ_0` (MPa) override from spreadsheet regression.
    #[serde(default, alias = "weibullSigma0OverrideMpa")]
    pub weibull_sigma0_override_mpa: Option<f64>,
}

fn default_nom_d() -> f64 {
    4.0
}
fn default_pitch() -> f64 {
    0.70
}
fn default_boss_od() -> f64 {
    10.0
}
fn default_substrate_material() -> String {
    "petg".into()
}
fn default_shock_g() -> f64 {
    30.0
}
fn default_num_fixings() -> usize {
    1
}
fn default_service_hours() -> f64 {
    1000.0
}
fn default_target_pf() -> f64 {
    1.0e-3
}
fn default_mu_thread() -> f64 {
    0.15
}
fn default_mu_head() -> f64 {
    0.16
}
fn default_mu_joint() -> f64 {
    0.25
}

impl Default for FixingEnvelopeInput {
    fn default() -> Self {
        Self {
            fixing_kind: FixingKind::SelfTappingScrew,
            nominal_d_mm: default_nom_d(),
            pitch_mm: default_pitch(),
            engagement_length_mm: default_engagement_mm(),
            boss_od_mm: default_boss_od(),
            substrate_material: default_substrate_material(),
            fastener_material: default_fastener_material(),
            vibration_g: default_vibration_g(),
            shock_g: default_shock_g(),
            payload_mass_kg: default_payload_mass_kg(),
            num_fixings: default_num_fixings(),
            delta_temp_c: 0.0,
            service_hours: default_service_hours(),
            target_failure_prob: default_target_pf(),
            mu_thread: default_mu_thread(),
            mu_head: default_mu_head(),
            mu_joint: default_mu_joint(),
            include_washer: false,
            associated_nut: AssociatedNutKind::None,
            weibull_m_override: None,
            weibull_sigma0_override_mpa: None,
        }
    }
}

/// VDI 2230 holdable axial and transverse shear force capacities per bolt/joint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FixingHoldableForces {
    pub axial_preload_at_rec_n: f64,
    pub residual_clamp_after_creep_n: f64,
    pub max_axial_load_before_gapping_n: f64,
    pub max_transverse_friction_slip_n: f64,
    pub max_shank_shear_pin_n: f64,
    pub max_thread_pullout_n: f64,
    pub max_underhead_bearing_n: f64,
    pub max_bolt_tensile_yield_n: f64,
    pub safe_working_axial_load_n: f64,
    pub safe_working_shear_load_n: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FixingEnvelopeReport {
    pub substrate_regime: SubstrateRegime,
    pub substrate_regime_label: String,
    /// Governing upper-bound failure criterion automatically selected for this material regime.
    pub governing_failure_mode: String,
    pub include_washer: bool,
    pub associated_nut: AssociatedNutKind,
    pub bearing_area_mm2: f64,
    pub bearing_pressure_at_rec_mpa: f64,
    /// Nut factor / Motosh torque arm `K_torque = T_preload / F_preload` (mm).
    pub motosh_torque_arm_mm: f64,
    /// Dimensionless nut factor `K_nut = K_torque / d`.
    pub nut_factor_k: f64,
    /// Self-tapping thread-forming or Nyloc prevailing torque `T_form` (N·m).
    pub thread_forming_torque_nm: f64,
    /// Minimum residual clamp force required to prevent Junker vibration slip & shock g-load separation (N).
    pub min_residual_clamp_n: f64,
    /// Initial assembly preload required to guarantee `min_residual_clamp_n` after creep + ΔT thermal loss (N).
    pub min_install_preload_n: f64,
    /// Fraction of initial preload retained after viscoplastic creep over `service_hours` (`0..1`).
    pub creep_retention_factor: f64,
    /// Preload shift `ΔF_thermal` due to differential thermal expansion over `ΔT` (N, negative = loss).
    pub thermal_preload_shift_n: f64,
    /// **Minimal installation torque `T_min` (N·m)** to prevent knock/vibration release.
    pub min_torque_nm: f64,
    /// Maximum allowable preload before triggering the governing failure mode (N).
    pub max_allowable_preload_n: f64,
    /// **Maximal installation torque `T_max` (N·m)** from the material's fracture/creep/yield limit.
    pub max_torque_nm: f64,
    /// Recommended nominal assembly torque `T_rec` inside `[T_min, T_max]` (N·m).
    pub recommended_torque_nm: f64,
    /// Recommended nominal assembly preload `F_rec` (N).
    pub recommended_preload_n: f64,
    /// Torque window safety ratio `T_max / T_min` (`> 1.0` means a feasible assembly window exists).
    pub torque_window_ratio: f64,
    /// Peak thread-root / boss hoop maximum principal stress `σ_1` at `T_rec` (MPa).
    pub peak_principal_stress_mpa: f64,
    /// Weibull fracture probability `P_f` at `T_rec` (`0..1`).
    pub weibull_failure_prob_at_rec: f64,
    /// Weibull modulus `m` used in the calculation.
    pub weibull_modulus_m: f64,
    /// Weibull characteristic strength scale `σ_0` (MPa) used in the calculation.
    pub weibull_scale_sigma0_mpa: f64,
    /// VDI 2230 axial gapping, friction slip, pin shear, thread pullout, and bearing crush capacities (N).
    pub holdable_forces: FixingHoldableForces,
    /// Individual failure mode torque limits (N·m) for side-by-side comparison.
    pub mode_limits_nm: FixingModeLimits,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FixingModeLimits {
    pub weibull_principal_fracture_nm: f64,
    pub boss_creep_or_hoop_split_nm: f64,
    pub substrate_thread_strip_nm: f64,
    pub fastener_core_yield_nm: f64,
    pub composite_interlaminar_peel_nm: f64,
    pub underhead_bearing_crush_nm: f64,
}

/// Evaluate the minimal and maximal torque envelope and holdable forces for a fixing.
pub fn evaluate_fixing_torque_envelope(inp: &FixingEnvelopeInput) -> FixingEnvelopeReport {
    evaluate_fixing_torque_envelope_with_doc(None, inp)
}

/// Evaluate the minimal and maximal torque envelope with optional document custom materials.
pub fn evaluate_fixing_torque_envelope_with_doc(
    doc: Option<&Document>,
    inp: &FixingEnvelopeInput,
) -> FixingEnvelopeReport {
    let sub_mat = resolve_fixing_material(doc, &inp.substrate_material, "petg");
    let fast_mat = resolve_fixing_material(doc, &inp.fastener_material, "ti64");
    let regime = SubstrateRegime::classify_material(&inp.substrate_material);

    let d = inp.nominal_d_mm.max(1.6);
    let p = inp.pitch_mm.max(0.25);
    let l_eng = inp.engagement_length_mm.max(1.5 * p);
    let boss_od = inp.boss_od_mm.max(d * 1.45);
    let n_fix = inp.num_fixings.max(1) as f64;
    let has_washer = inp.include_washer || inp.fixing_kind == FixingKind::Washer;
    let nut_kind = inp.associated_nut;

    let std_size = find_fixing_size_by_diameter(d);
    let d_hole = std_size.clearance_d;
    let d_head_outer = if inp.fixing_kind == FixingKind::HexHeadScrew {
        std_size.hex_flats_s
    } else {
        std_size.cap_head_d.max(d * 1.5)
    };
    let d_washer_outer = std_size.washer_od.max(d_head_outer * 1.35);
    let t_washer = std_size.washer_t;

    let bearing_outer_d = if has_washer {
        d_washer_outer
    } else {
        d_head_outer
    };
    let bearing_area_mm2 =
        ((PI / 4.0) * (bearing_outer_d * bearing_outer_d - d_hole * d_hole)).max(2.5);
    let bare_head_area_mm2 =
        ((PI / 4.0) * (d_head_outer * d_head_outer - d_hole * d_hole)).max(2.0);
    let washer_area_ratio = bearing_area_mm2 / bare_head_area_mm2;

    let d_pitch = d - 0.64952 * p;
    let d_minor = d - 1.22687 * p;
    let a_tensile = (PI / 4.0) * (0.5 * (d_pitch + d_minor)).powi(2);
    let r_t = 0.5 * d_pitch;
    let r_h = if has_washer {
        0.25 * (d_head_outer + d_hole) * 1.04
    } else {
        0.625 * d
    };
    let effective_mu_head = if has_washer {
        if matches!(
            regime,
            SubstrateRegime::PlasticBoss | SubstrateRegime::WovenComposite
        ) {
            inp.mu_head.min(0.145)
        } else {
            inp.mu_head * 0.92
        }
    } else {
        inp.mu_head
    };

    let alpha_rad = if inp.fixing_kind == FixingKind::SelfTappingScrew
        && regime == SubstrateRegime::PlasticBoss
        && nut_kind == AssociatedNutKind::None
    {
        15.0_f64.to_radians()
    } else {
        30.0_f64.to_radians()
    };

    let lead_term_mm = p / (2.0 * PI);
    let thread_friction_arm_mm = inp.mu_thread.max(0.02) * r_t / alpha_rad.cos();
    let head_friction_arm_mm = effective_mu_head.max(0.02) * r_h;
    let motosh_arm_mm = lead_term_mm + thread_friction_arm_mm + head_friction_arm_mm;
    let nut_factor_k = motosh_arm_mm / d;

    let mut thread_forming_torque_nm =
        if inp.fixing_kind == FixingKind::SelfTappingScrew && nut_kind == AssociatedNutKind::None {
            let radial_bite = (0.18 * d).max(0.25);
            let form_shear_mpa = 0.42 * sub_mat.yield_strength;
            let form_nmm = 0.085 * form_shear_mpa * radial_bite * d * l_eng * (p / d).sqrt();
            (form_nmm * 1.0e-3).max(0.02)
        } else {
            0.0
        };
    if inp.fixing_kind == FixingKind::NylocNut || nut_kind == AssociatedNutKind::NylocNut {
        thread_forming_torque_nm += (0.018 * d.powf(1.85)).max(0.05);
    }

    let eff_clamped_len = l_eng + if has_washer { t_washer } else { 0.0 };
    let k_bolt = fast_mat.e * a_tensile / eff_clamped_len.max(2.0);
    let a_clamped =
        ((PI / 4.0) * (boss_od.max(bearing_outer_d).powi(2) - d.powi(2))).max(2.0 * a_tensile);
    let k_member = sub_mat.e * a_clamped / eff_clamped_len.max(2.0);
    let phi_k = (k_bolt / (k_bolt + k_member)).clamp(0.03, 0.85);

    let creep_retention_factor = match regime {
        SubstrateRegime::PlasticBoss => {
            let t_hr = inp.service_hours.max(0.1);
            let creep_rate = if has_washer { 0.031 } else { 0.048 };
            (1.0 - creep_rate * (1.0 + t_hr).ln()).clamp(0.52, 0.98)
        }
        SubstrateRegime::WovenComposite => {
            let t_hr = inp.service_hours.max(0.1);
            let creep_rate = if has_washer { 0.0075 } else { 0.012 };
            (1.0 - creep_rate * (1.0 + t_hr).ln()).clamp(0.85, 0.995)
        }
        _ => 0.98,
    };

    let alpha_sub = sub_mat.alpha.unwrap_or(20.0e-6);
    let alpha_fast = fast_mat.alpha.unwrap_or(10.0e-6);
    let k_eq = (k_bolt * k_member) / (k_bolt + k_member).max(1.0);
    let thermal_preload_shift_n =
        (alpha_sub - alpha_fast) * inp.delta_temp_c * eff_clamped_len * k_eq;

    let g0 = 9.80665;
    let f_trans_vib = inp.payload_mass_kg.max(0.01) * inp.vibration_g.max(0.1) * g0 / n_fix;
    let f_axial_shock = inp.payload_mass_kg.max(0.01) * inp.shock_g.max(0.1) * g0 / n_fix;

    let vib_factor =
        if nut_kind == AssociatedNutKind::NylocNut || inp.fixing_kind == FixingKind::NylocNut {
            0.92
        } else {
            1.15
        };
    let min_residual_clamp_n = (vib_factor * f_trans_vib / inp.mu_joint.max(0.08))
        + (1.25 * f_axial_shock * (1.0 - phi_k))
        + 12.0 * (d / 4.0).powi(2);

    let thermal_loss_n = (-thermal_preload_shift_n).max(0.0);
    let min_install_preload_n =
        (min_residual_clamp_n + thermal_loss_n) / creep_retention_factor.max(0.4);

    let min_torque_nm = min_install_preload_n * motosh_arm_mm * 1.0e-3 + thread_forming_torque_nm;

    let (default_m, default_sigma0) = match regime {
        SubstrateRegime::PlasticBoss => (8.2, sub_mat.yield_strength * 1.36),
        SubstrateRegime::MetalHousing => (16.5, sub_mat.yield_strength * 1.45),
        SubstrateRegime::BrittleWeibullOptics => (9.0, sub_mat.yield_strength),
        SubstrateRegime::WovenComposite => (11.2, sub_mat.yield_strength * 0.95),
    };
    let weibull_m = inp.weibull_m_override.unwrap_or(default_m).clamp(1.5, 45.0);
    let weibull_sigma0 = inp
        .weibull_sigma0_override_mpa
        .unwrap_or(default_sigma0)
        .max(10.0);

    let v_eff_ratio = ((PI * d * p * 1.5) / 100.0).clamp(0.10, 2.5);
    let pf_allow = inp.target_failure_prob.clamp(1.0e-9, 0.50);
    let weibull_ln_term = (-(1.0 - pf_allow).ln() / v_eff_ratio).max(1.0e-12);
    let sigma1_weibull_allow_mpa = weibull_sigma0 * weibull_ln_term.powf(1.0 / weibull_m);

    let r_i = 0.5 * d;
    let r_o = 0.5 * boss_od.max(d * 1.35);
    let lame_hoop_factor = (r_o.powi(2) + r_i.powi(2)) / (r_o.powi(2) - r_i.powi(2)).max(0.5);
    let radial_wedge = if nut_kind != AssociatedNutKind::None {
        0.08
    } else {
        alpha_rad.tan().max(0.26)
    };
    let boss_wall_area = (PI * 0.25 * (boss_od.powi(2) - d.powi(2))).max(8.0);
    let bearing_area_thread = (PI * d_pitch * l_eng * 0.72
        + 0.45 * boss_wall_area
        + if has_washer {
            0.55 * bearing_area_mm2
        } else {
            0.0
        })
    .max(12.0);
    let c_sigma1_per_n = (1.35 * radial_wedge * lame_hoop_factor + 2.40) / bearing_area_thread;

    let f_max_weibull = sigma1_weibull_allow_mpa / c_sigma1_per_n;
    let weibull_principal_fracture_nm =
        f_max_weibull * motosh_arm_mm * 1.0e-3 + thread_forming_torque_nm;

    let (sz_allow, ss_allow) = sub_mat.effective_layer_strengths();
    let allowable_bearing_stress_mpa = match regime {
        SubstrateRegime::PlasticBoss => 0.82 * sub_mat.yield_strength,
        SubstrateRegime::WovenComposite => 1.15 * sz_allow,
        _ => 1.25 * sub_mat.yield_strength,
    };
    let f_max_underhead_bearing = allowable_bearing_stress_mpa * bearing_area_mm2;
    let underhead_bearing_crush_nm =
        f_max_underhead_bearing * motosh_arm_mm * 1.0e-3 + thread_forming_torque_nm;

    let sigma_creep_allow_mpa = 0.75 * sub_mat.yield_strength;
    let c_hoop_per_n = (0.45 * radial_wedge * lame_hoop_factor + 0.55) / bearing_area_thread;
    let f_max_boss_creep =
        (sigma_creep_allow_mpa / c_hoop_per_n).min(f_max_underhead_bearing * 1.15);
    let boss_creep_or_hoop_split_nm =
        f_max_boss_creep * motosh_arm_mm * 1.0e-3 + thread_forming_torque_nm;

    let effective_thread_shear_strength = if nut_kind != AssociatedNutKind::None {
        0.577 * sub_mat.yield_strength.max(fast_mat.yield_strength * 0.75)
    } else {
        0.577 * sub_mat.yield_strength
    };
    let effective_thread_len = if nut_kind != AssociatedNutKind::None {
        l_eng.max(std_size.nut_h * 1.15)
    } else {
        l_eng
    };
    let a_shear_sub = PI * d * effective_thread_len * 0.62;
    let f_max_thread_strip = effective_thread_shear_strength * a_shear_sub;
    let substrate_thread_strip_nm =
        f_max_thread_strip * motosh_arm_mm * 1.0e-3 + thread_forming_torque_nm;

    let d_s = (4.0 * a_tensile / PI).sqrt();
    let t_thread_arm = lead_term_mm + thread_friction_arm_mm;
    let tau_per_n = 16.0 * t_thread_arm / (PI * d_s.powi(3));
    let sigma_ax_per_n = 1.0 / a_tensile;
    let vm_per_n = (sigma_ax_per_n.powi(2) + 3.0 * tau_per_n.powi(2)).sqrt();
    let f_max_fastener_yield = (0.90 * fast_mat.yield_strength) / vm_per_n;
    let fastener_core_yield_nm =
        f_max_fastener_yield * motosh_arm_mm * 1.0e-3 + thread_forming_torque_nm;

    let a_washer_bearing = bearing_area_mm2.max(4.0);
    let peel_per_n = ((0.35 / (a_washer_bearing * sz_allow)).powi(2)
        + (1.0 / (bearing_area_thread * ss_allow)).powi(2))
    .sqrt();
    let f_max_composite_peel = 1.0 / peel_per_n.max(1.0e-6);
    let composite_interlaminar_peel_nm =
        f_max_composite_peel * motosh_arm_mm * 1.0e-3 + thread_forming_torque_nm;

    let nut_strip_label = match nut_kind {
        AssociatedNutKind::NylocNut => "Mated Nyloc Nut Thread Shear Limit",
        AssociatedNutKind::HexNut => "Mated Hex Nut Thread Shear Limit",
        AssociatedNutKind::None => "Polymer Internal Thread Shear Stripping",
    };
    let metal_strip_label = match nut_kind {
        AssociatedNutKind::NylocNut => "Mated Nyloc Nut Thread Shear Limit",
        AssociatedNutKind::HexNut => "Mated Hex Nut Thread Shear Limit",
        AssociatedNutKind::None => "Internal Housing Thread Shear Stripping",
    };

    let (governing_failure_mode, max_torque_nm, max_allowable_preload_n) = match regime {
        SubstrateRegime::PlasticBoss => {
            let creep_label = if has_washer {
                "Viscoplastic Boss Hoop Creep / Radial Split (Washer-Distributed)"
            } else {
                "Underhead Bearing Crush & Viscoplastic Boss Creep (No Washer)"
            };
            let candidates = [
                (
                    creep_label,
                    boss_creep_or_hoop_split_nm.min(underhead_bearing_crush_nm),
                    f_max_boss_creep.min(f_max_underhead_bearing),
                ),
                (
                    nut_strip_label,
                    substrate_thread_strip_nm,
                    f_max_thread_strip,
                ),
                (
                    "Weibull Principal-Stress Boss Fracture",
                    weibull_principal_fracture_nm,
                    f_max_weibull,
                ),
            ];
            candidates
                .into_iter()
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .unwrap()
        }
        SubstrateRegime::MetalHousing => {
            let candidates = [
                (
                    "Fastener Core Combined Tension–Torsion Yield (VDI 2230)",
                    fastener_core_yield_nm,
                    f_max_fastener_yield,
                ),
                (
                    metal_strip_label,
                    substrate_thread_strip_nm,
                    f_max_thread_strip,
                ),
                (
                    "Underhead Bearing Yield Limit",
                    underhead_bearing_crush_nm,
                    f_max_underhead_bearing,
                ),
                (
                    "Weibull Principal-Stress Root Fracture",
                    weibull_principal_fracture_nm,
                    f_max_weibull,
                ),
            ];
            candidates
                .into_iter()
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .unwrap()
        }
        SubstrateRegime::BrittleWeibullOptics => {
            let candidates = [
                (
                    "Weibull Principal-Stress Brittle Fracture Statistics",
                    weibull_principal_fracture_nm,
                    f_max_weibull,
                ),
                (
                    "Fastener Core Combined Tension–Torsion Yield (VDI 2230)",
                    fastener_core_yield_nm,
                    f_max_fastener_yield,
                ),
            ];
            candidates
                .into_iter()
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .unwrap()
        }
        SubstrateRegime::WovenComposite => {
            let peel_label = if has_washer {
                "Tsai–Wu Interlaminar Peel & Washer-Spread Bearing"
            } else {
                "Tsai–Wu Underhead Bearing Crush & Interlaminar Delamination (No Washer)"
            };
            let candidates = [
                (
                    peel_label,
                    composite_interlaminar_peel_nm.min(underhead_bearing_crush_nm),
                    f_max_composite_peel.min(f_max_underhead_bearing),
                ),
                (
                    "Weibull Principal-Stress Matrix Micro-Cracking",
                    weibull_principal_fracture_nm,
                    f_max_weibull,
                ),
            ];
            candidates
                .into_iter()
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .unwrap()
        }
    };

    let recommended_torque_nm = if max_torque_nm > min_torque_nm {
        min_torque_nm + 0.55 * (max_torque_nm - min_torque_nm)
    } else {
        0.5 * (min_torque_nm + max_torque_nm)
    };
    let recommended_preload_n =
        ((recommended_torque_nm - thread_forming_torque_nm).max(0.01) * 1.0e3) / motosh_arm_mm;

    let bearing_pressure_at_rec_mpa = recommended_preload_n / bearing_area_mm2.max(1.0);
    let peak_principal_stress_mpa =
        (recommended_preload_n * c_sigma1_per_n) / washer_area_ratio.powf(0.18);
    let weibull_failure_prob_at_rec =
        1.0 - (-v_eff_ratio * (peak_principal_stress_mpa / weibull_sigma0).powf(weibull_m)).exp();

    let residual_clamp_after_creep_n = (recommended_preload_n * creep_retention_factor
        + thermal_preload_shift_n.min(0.0))
    .max(0.0);
    let phi_effective = (0.35 * phi_k).min(0.32);
    let max_axial_load_before_gapping_n =
        residual_clamp_after_creep_n / (1.0 - phi_effective).max(0.68);
    let max_transverse_friction_slip_n = residual_clamp_after_creep_n * inp.mu_joint.max(0.08);
    let max_shank_shear_pin_n = 0.6 * (0.577 * fast_mat.yield_strength) * a_tensile;
    let max_bolt_tensile_yield_n = 0.9 * fast_mat.yield_strength * a_tensile;
    let max_thread_pullout_n = f_max_thread_strip;
    let max_underhead_bearing_n = f_max_underhead_bearing;
    let safe_working_axial_load_n = max_axial_load_before_gapping_n
        .min(max_thread_pullout_n / 1.5)
        .min(max_underhead_bearing_n / 1.35)
        .min(max_bolt_tensile_yield_n / 1.5);
    let safe_working_shear_load_n = max_transverse_friction_slip_n / 1.15;

    FixingEnvelopeReport {
        substrate_regime: regime,
        substrate_regime_label: regime.label().into(),
        governing_failure_mode: governing_failure_mode.into(),
        include_washer: has_washer,
        associated_nut: nut_kind,
        bearing_area_mm2,
        bearing_pressure_at_rec_mpa,
        motosh_torque_arm_mm: motosh_arm_mm,
        nut_factor_k,
        thread_forming_torque_nm,
        min_residual_clamp_n,
        min_install_preload_n,
        creep_retention_factor,
        thermal_preload_shift_n,
        min_torque_nm,
        max_allowable_preload_n,
        max_torque_nm,
        recommended_torque_nm,
        recommended_preload_n,
        torque_window_ratio: max_torque_nm / min_torque_nm.max(1.0e-6),
        peak_principal_stress_mpa,
        weibull_failure_prob_at_rec,
        weibull_modulus_m: weibull_m,
        weibull_scale_sigma0_mpa: weibull_sigma0,
        holdable_forces: FixingHoldableForces {
            axial_preload_at_rec_n: recommended_preload_n,
            residual_clamp_after_creep_n,
            max_axial_load_before_gapping_n,
            max_transverse_friction_slip_n,
            max_shank_shear_pin_n,
            max_thread_pullout_n,
            max_underhead_bearing_n,
            max_bolt_tensile_yield_n,
            safe_working_axial_load_n,
            safe_working_shear_load_n,
        },
        mode_limits_nm: FixingModeLimits {
            weibull_principal_fracture_nm,
            boss_creep_or_hoop_split_nm,
            substrate_thread_strip_nm,
            fastener_core_yield_nm,
            composite_interlaminar_peel_nm,
            underhead_bearing_crush_nm,
        },
    }
}

/// One row of the empirical failure probability spreadsheet table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FailureSampleRow {
    #[serde(alias = "id")]
    pub sample_id: String,
    /// Applied installation / stripped / fracture torque `T_i` (N·m).
    #[serde(alias = "torqueNm")]
    pub torque_nm: f64,
    /// Peak principal or hoop stress `σ_{1,i}` (MPa) at this torque.
    #[serde(alias = "principalStressMpa")]
    pub principal_stress_mpa: f64,
    /// Empirical cumulative failure probability `P_f` in `(0, 1)`. When `None`, Bernard's
    /// median rank `F_i = (i - 0.3) / (N + 0.4)` is assigned after sorting by stress.
    #[serde(default, alias = "failureProb")]
    pub failure_prob: Option<f64>,
    /// Observed failure mode note (e.g. `"Boss hoop crack"`, `"Thread root strip"`).
    #[serde(default, alias = "modeNote")]
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FittedFailurePoint {
    pub id: String,
    pub torque_nm: f64,
    pub stress_mpa: f64,
    pub pf: f64,
    pub ln_stress: f64,
    pub ln_torque: f64,
    pub weibull_y: f64,
    pub mode_note: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FailureDistributionFit {
    /// Fitted 2-parameter Weibull modulus `m` (slope of `ln(ln(1/(1-P_f)))` vs `ln(σ_1)`).
    pub weibull_modulus_m: f64,
    /// Fitted Weibull characteristic stress scale `σ_0` (MPa) at `P_f = 63.21%`.
    pub weibull_scale_sigma0_mpa: f64,
    /// Fitted Weibull characteristic torque scale `T_0` (N·m) at `P_f = 63.21%`.
    pub weibull_scale_t0_nm: f64,
    /// Coefficient of determination `R²` (`0..1`) of the Weibull linear regression.
    pub weibull_r2: f64,
    /// Log-normal mean log-stress `μ_ln` and standard deviation `s_ln`.
    pub lognormal_mu_ln: f64,
    pub lognormal_sigma_ln: f64,
    /// Maximum allowable principal stress `σ_max(P_f,target)` (MPa) at `target_pf`.
    pub allowable_stress_at_target_pf_mpa: f64,
    /// Maximum allowable torque `T_max(P_f,target)` (N·m) at `target_pf`.
    pub allowable_torque_at_target_pf_nm: f64,
    /// Sorted coupon regression points in Weibull linearized coordinates.
    pub points: Vec<FittedFailurePoint>,
}

/// Fit a 2-parameter Weibull distribution (`P_f = 1 - exp(-(σ_1 / σ_0)^m)`) and Log-Normal
/// distribution to a table of empirical failure coupon samples.
pub fn fit_failure_distribution(
    samples: &[FailureSampleRow],
    target_pf: f64,
) -> FailureDistributionFit {
    let mut sorted: Vec<FailureSampleRow> = samples
        .iter()
        .filter(|s| {
            s.principal_stress_mpa.is_finite()
                && s.torque_nm.is_finite()
                && s.principal_stress_mpa > 1.0e-3
                && s.torque_nm > 1.0e-4
        })
        .cloned()
        .collect();
    sorted.sort_by(|a, b| {
        a.principal_stress_mpa
            .partial_cmp(&b.principal_stress_mpa)
            .unwrap()
    });

    let n = sorted.len();
    if n < 2 {
        return FailureDistributionFit {
            weibull_modulus_m: 8.0,
            weibull_scale_sigma0_mpa: 60.0,
            weibull_scale_t0_nm: 1.8,
            weibull_r2: 1.0,
            lognormal_mu_ln: 60.0_f64.ln(),
            lognormal_sigma_ln: 0.15,
            allowable_stress_at_target_pf_mpa: 35.0,
            allowable_torque_at_target_pf_nm: 1.0,
            points: Vec::new(),
        };
    }

    let mut points = Vec::with_capacity(n);
    for (idx, row) in sorted.iter().enumerate() {
        let pf = row
            .failure_prob
            .unwrap_or_else(|| ((idx as f64 + 1.0) - 0.3) / (n as f64 + 0.4))
            .clamp(1.0e-6, 1.0 - 1.0e-6);
        let ln_stress = row.principal_stress_mpa.ln();
        let ln_torque = row.torque_nm.ln();
        let weibull_y = (-(1.0 - pf).ln()).ln();
        points.push(FittedFailurePoint {
            id: row.sample_id.clone(),
            torque_nm: row.torque_nm,
            stress_mpa: row.principal_stress_mpa,
            pf,
            ln_stress,
            ln_torque,
            weibull_y,
            mode_note: row.note.clone(),
        });
    }

    let nf = n as f64;
    let x_mean = points.iter().map(|p| p.ln_stress).sum::<f64>() / nf;
    let xt_mean = points.iter().map(|p| p.ln_torque).sum::<f64>() / nf;
    let y_mean = points.iter().map(|p| p.weibull_y).sum::<f64>() / nf;

    let mut s_xx = 0.0;
    let mut s_xy = 0.0;
    let mut s_yy = 0.0;
    let mut s_xtx = 0.0;
    let mut s_xty = 0.0;

    for pt in &points {
        let dx = pt.ln_stress - x_mean;
        let dxt = pt.ln_torque - xt_mean;
        let dy = pt.weibull_y - y_mean;
        s_xx += dx * dx;
        s_xy += dx * dy;
        s_yy += dy * dy;
        s_xtx += dxt * dxt;
        s_xty += dxt * dy;
    }

    let m = (s_xy / s_xx.max(1.0e-12)).clamp(1.2, 60.0);
    let intercept = y_mean - m * x_mean;
    let sigma0 = (-intercept / m).exp();

    let mt = (s_xty / s_xtx.max(1.0e-12)).clamp(1.2, 60.0);
    let intercept_t = y_mean - mt * xt_mean;
    let t0 = (-intercept_t / mt).exp();

    let r2 = if s_yy > 1.0e-12 && s_xx > 1.0e-12 {
        ((s_xy * s_xy) / (s_xx * s_yy)).clamp(0.0, 1.0)
    } else {
        1.0
    };

    let sigma_ln = (s_xx / (nf - 1.0).max(1.0)).sqrt();

    let pf_t = target_pf.clamp(1.0e-9, 0.50);
    let weibull_factor = (-(1.0 - pf_t).ln()).powf(1.0 / m);
    let allowable_stress = sigma0 * weibull_factor;
    let allowable_torque = t0 * (-(1.0 - pf_t).ln()).powf(1.0 / mt);

    FailureDistributionFit {
        weibull_modulus_m: m,
        weibull_scale_sigma0_mpa: sigma0,
        weibull_scale_t0_nm: t0,
        weibull_r2: r2,
        lognormal_mu_ln: x_mean,
        lognormal_sigma_ln: sigma_ln,
        allowable_stress_at_target_pf_mpa: allowable_stress,
        allowable_torque_at_target_pf_nm: allowable_torque,
        points,
    }
}

/// VDI 2251 tapered cantilever hook-clip snap-fit mechanics report.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapFitMechanicsReport {
    pub beam_length_mm: f64,
    pub beam_width_mm: f64,
    pub beam_thickness_mm: f64,
    pub undercut_depth_mm: f64,
    pub insertion_force_push_n: f64,
    pub retention_force_pull_n: f64,
    pub root_flexural_strain_pct: f64,
    pub allowable_strain_pct: f64,
    pub strain_safety_factor: f64,
}

/// Evaluate VDI 2251 tapered cantilever snap-fit insertion force, retention pull-out force,
/// and root flexural strain margin.
pub fn evaluate_snap_fit_mechanics(
    doc: Option<&Document>,
    nominal_d_mm: f64,
    engagement_length_mm: f64,
    substrate_material: &str,
    mu_joint: f64,
) -> SnapFitMechanicsReport {
    let sub_mat = resolve_fixing_material(doc, substrate_material, "petg");
    let beam_w = (nominal_d_mm * 1.6).max(5.0);
    let beam_t = (nominal_d_mm * 0.45).max(1.6);
    let beam_l = engagement_length_mm.max(10.0);
    let undercut_y = (beam_t * 0.45).max(0.75);

    let k_arm_n_per_mm = (sub_mat.e * beam_w * beam_t.powi(3)) / (4.0 * beam_l.powi(3));
    let f_deflect_n = k_arm_n_per_mm * undercut_y;

    let alpha_entry = 30.0_f64.to_radians();
    let beta_ret = 65.0_f64.to_radians();
    let mu = mu_joint.clamp(0.08, 0.45);

    let wedge_push = (alpha_entry.tan() + mu) / (1.0 - mu * alpha_entry.tan()).max(0.15);
    let wedge_pull = (beta_ret.tan() + mu) / (1.0 - mu * beta_ret.tan()).max(0.15);

    let insertion_force_push_n = f_deflect_n * wedge_push;
    let retention_force_pull_n = f_deflect_n * wedge_pull;

    let root_flexural_strain = (1.15 * beam_t * undercut_y) / (beam_l * beam_l);
    let allowable_strain = match substrate_material.trim().to_ascii_lowercase().as_str() {
        "pa12" | "nylon_pa66" | "pom_delrin" => 0.032,
        "petg" | "abs" | "pla" | "pc_lexan" | "peek" | "peek_cf30" => 0.022,
        _ => 0.006,
    };

    SnapFitMechanicsReport {
        beam_length_mm: beam_l,
        beam_width_mm: beam_w,
        beam_thickness_mm: beam_t,
        undercut_depth_mm: undercut_y,
        insertion_force_push_n,
        retention_force_pull_n,
        root_flexural_strain_pct: root_flexural_strain * 100.0,
        allowable_strain_pct: allowable_strain * 100.0,
        strain_safety_factor: allowable_strain / root_flexural_strain.max(1.0e-6),
    }
}

/// Open-access public standard provenance and dual-mode benchmark verification
/// (a: Quick Standard Lookup vs b: First-Principles A-Priori Rederivation).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpenStandardBenchmark {
    pub id: String,
    pub standard_code: String,
    pub iso_cross_ref: String,
    pub open_url: String,
    pub title: String,
    pub quick_standard_rule: String,
    pub apriori_derivation: String,
    pub metric_label: String,
    pub standard_value: f64,
    pub apriori_value: f64,
    pub discrepancy_pct: f64,
    pub regime_insight: String,
}

/// Evaluate the 4 canonical open-access standard benchmarks (FED-STD-H28, NASA-STD-5020B,
/// NIST/SEMATECH 8.1.6.2, and NASA-HDBK-7005 CARES/Life) with default reference coupons.
pub fn evaluate_open_standard_benchmarks(inp: &FixingEnvelopeInput) -> Vec<OpenStandardBenchmark> {
    evaluate_open_standard_benchmarks_with_samples(inp, None, 1.0e-6)
}

/// Evaluate the 4 canonical open-access standard benchmarks, optionally fitting custom
/// coupon spreadsheet rows at `target_pf`.
pub fn evaluate_open_standard_benchmarks_with_samples(
    inp: &FixingEnvelopeInput,
    active_samples: Option<&[FailureSampleRow]>,
    target_pf: f64,
) -> Vec<OpenStandardBenchmark> {
    let d = inp.nominal_d_mm.max(1.5);
    let p = inp.pitch_mm.max(0.25);
    let l_eng = inp.engagement_length_mm.max(1.5 * p);
    let mu_t = inp.mu_thread.clamp(0.04, 0.60);
    let mu_h = inp.mu_head.clamp(0.04, 0.60);

    // 1. FED-STD-H28 / Section 2B (ASME B1.13M / ISO 68-1 / ISO 898-1)
    let as_standard_mm2 = 0.25 * PI * (d - 0.938_194 * p).powi(2);
    let h_triangle = (3.0_f64.sqrt() / 2.0) * p;
    let d2_apriori = d - 0.75 * h_triangle;
    let d3_apriori = d - (17.0 / 12.0) * h_triangle;
    let ds_apriori = 0.5 * (d2_apriori + d3_apriori);
    let as_apriori_mm2 = 0.25 * PI * ds_apriori * ds_apriori;
    let err_b1 = ((as_apriori_mm2 - as_standard_mm2).abs() / as_standard_mm2) * 100.0;

    // 2. NASA-STD-5020B Clause 4.2 / Appendix A & MIL-HDBK-60
    let env = evaluate_fixing_torque_envelope(inp);
    let preload_ref_n = env.recommended_preload_n.max(100.0);
    let k_std = if (mu_t - 0.14).abs() < 0.03 {
        0.193
    } else if mu_t < 0.12 {
        0.165
    } else {
        0.215
    };
    let t_std_nm = k_std * (d * 1.0e-3) * preload_ref_n + env.thread_forming_torque_nm;
    let alpha_n = if inp.fixing_kind == FixingKind::SelfTappingScrew {
        15.0_f64.to_radians()
    } else {
        30.0_f64.to_radians()
    };
    let r2 = 0.5 * d2_apriori;
    let rb = 0.65 * d;
    let t_apriori_nm =
        preload_ref_n * 1.0e-3 * (p / (2.0 * PI) + (mu_t * r2) / alpha_n.cos() + mu_h * rb)
            + env.thread_forming_torque_nm;
    let err_b2 = ((t_apriori_nm - t_std_nm).abs() / t_std_nm.max(1.0e-6)) * 100.0;

    // 3. NIST/SEMATECH e-Handbook Section 8.1.6.2 (Weibull & Bernard Median Ranks)
    let (nist_fit, sigma0_ref) = match active_samples.filter(|s| s.len() >= 2) {
        Some(custom) => {
            let sub_mat = resolve_fixing_material(None, &inp.substrate_material, "petg");
            let sig_cat = catalog_weibull_sigma0_mpa(&inp.substrate_material, &sub_mat);
            (fit_failure_distribution(custom, target_pf), sig_cat)
        }
        None => {
            let nist_coupons = [
                (0.083, "1st thread root flaw"),
                (0.190, "1st thread root flaw"),
                (0.310, "Subsurface pore cone crack"),
                (0.440, "Thread flank Hertzian cone"),
                (0.560, "Spotface fillet ring crack"),
                (0.690, "1st thread root cleavage"),
                (0.810, "Boss radial cleavage"),
                (0.920, "Fastener head seat burst"),
            ]
            .iter()
            .enumerate()
            .map(|(i, &(pf, note))| {
                let factor = (-(1.0_f64 - pf).ln()).powf(1.0 / 10.5);
                FailureSampleRow {
                    sample_id: format!("NIST-{}", i + 1),
                    torque_nm: 5.78 * factor,
                    principal_stress_mpa: 340.0 * factor,
                    failure_prob: Some(pf),
                    note: note.into(),
                }
            })
            .collect::<Vec<_>>();
            (fit_failure_distribution(&nist_coupons, target_pf), 340.0)
        }
    };
    let err_b3 = ((nist_fit.weibull_scale_sigma0_mpa - sigma0_ref).abs() / sigma0_ref) * 100.0;

    // 4. NASA-HDBK-7005 & CARES/Life (NASA TM-106316) + Sopwith (1948)
    let sopwith_handbook_factor = 2.85;
    let lambda = (l_eng / (1.05 * d)).clamp(0.6, 4.5);
    let sopwith_apriori_factor = (lambda / lambda.tanh()).clamp(1.35, 3.8);
    let err_b4 = ((sopwith_apriori_factor - sopwith_handbook_factor).abs()
        / sopwith_handbook_factor)
        * 100.0;

    vec![
        OpenStandardBenchmark {
            id: "fed_std_h28_stress_area".into(),
            standard_code: "FED-STD-H28 §2B / ASME B1.13M".into(),
            iso_cross_ref: "ISO 68-1 / ISO 898-1 §9.1.6".into(),
            open_url: "https://assist.dla.mil/".into(),
            title: "Metric 60° Thread Tensile Stress Area A_s".into(),
            quick_standard_rule:
                "Tabulated handbook formula: A_s = (π/4)·(d − 0.938194·P)²".into(),
            apriori_derivation:
                "Derived from 60° fundamental triangle height H = (√3/2)·P averaging pitch diameter d₂ = d − (3√3/8)·P and minor root diameter d₃ = d − (17√3/24)·P → d_s = d − (13√3/24)·P = d − 0.938194·P".into(),
            metric_label: format!("A_s (mm²) at d={:.1} mm, P={:.2} mm", d, p),
            standard_value: as_standard_mm2,
            apriori_value: as_apriori_mm2,
            discrepancy_pct: err_b1,
            regime_insight:
                "Exact geometric identity (0.000% discrepancy): the 0.938194 constant in ISO 898-1 and FED-STD-H28 equals 13√3/24 from the 60° V-thread truncation geometry.".into(),
        },
        OpenStandardBenchmark {
            id: "nasa_std_5020b_torque_preload".into(),
            standard_code: "NASA-STD-5020B §4.2 / MIL-HDBK-60".into(),
            iso_cross_ref: "VDI 2230 Part 1 §5.4 / ISO 16047".into(),
            open_url: "https://standards.nasa.gov/standard/NASA/NASA-STD-5020".into(),
            title: "Torque–Preload Nut Factor vs. Motosh Inclined-Plane Integral".into(),
            quick_standard_rule:
                format!("Short-form handbook rule: T = K·d·F_preload + T_prev (K={k_std:.3})"),
            apriori_derivation:
                "3-surface virtual work integral: T = F·[P/(2π) + μ_t·r₂/cos(α_n) + μ_h·r_b] + T_form over lead angle β, flank angle α_n (15° Plastite vs 30° ISO), and annular bearing radius r_b".into(),
            metric_label: format!("T_rec (N·m) at F={:.0} N", preload_ref_n),
            standard_value: t_std_nm,
            apriori_value: t_apriori_nm,
            discrepancy_pct: err_b2,
            regime_insight:
                "Standard K = 0.20 works near μ ≈ 0.15 on 60° threads, but diverges on 30° Plastite self-tapping threads (cos 15° vs cos 30° reduces flank wedging by 10.4%) or large washer bearing radii.".into(),
        },
        OpenStandardBenchmark {
            id: "nist_sematech_8162_weibull".into(),
            standard_code: "NIST/SEMATECH e-Handbook §8.1.6.2".into(),
            iso_cross_ref: "ISO 20501 / ASTM C1239 (Weibull Ceramics)".into(),
            open_url: "https://www.itl.nist.gov/div898/handbook/apr/section1/apr162.htm".into(),
            title: "Weibull Extreme-Value Parameter Recovery".into(),
            quick_standard_rule:
                format!("Reference coupon scale σ₀ = {sigma0_ref:.1} MPa"),
            apriori_derivation:
                "Extreme-value weakest-link Poisson flaw integral P_f = 1 − exp(−(σ₁/σ₀)^m) linearized via ln(−ln(1−P_f)) = m·ln(σ₁) − m·ln(σ₀) with Bernard's median rank F_i = (i−0.3)/(N+0.4)".into(),
            metric_label: format!(
                "Recovered Scale σ₀ (MPa) (m={:.2}, R²={:.2}%)",
                nist_fit.weibull_modulus_m,
                nist_fit.weibull_r2 * 100.0
            ),
            standard_value: sigma0_ref,
            apriori_value: nist_fit.weibull_scale_sigma0_mpa,
            discrepancy_pct: err_b3,
            regime_insight: format!(
                "Linear regression on the coupon rows recovers σ₀ = {:.1} MPa and m = {:.2} (R² = {:.4}), giving allowable stress {:.1} MPa at target P_f = {:.0e}.",
                nist_fit.weibull_scale_sigma0_mpa,
                nist_fit.weibull_modulus_m,
                nist_fit.weibull_r2,
                nist_fit.allowable_stress_at_target_pf_mpa,
                target_pf
            ),
        },
        OpenStandardBenchmark {
            id: "nasa_hdbk_7005_sopwith_thread_load".into(),
            standard_code: "NASA-HDBK-7005 / CARES (TM-106316)".into(),
            iso_cross_ref: "VDI 2230 §5.1 / Sopwith (1948)".into(),
            open_url: "https://ntrs.nasa.gov/citations/20010021161".into(),
            title: "First-Engaged Thread Root Load Concentration Y₁".into(),
            quick_standard_rule:
                "Empirical design rule: 1st thread carries ~2.85× average thread load".into(),
            apriori_derivation:
                "Coupled axial bolt-stretching vs nut-compression differential equation d²F/dz² − (λ/L_eng)²·F = 0 yielding hyperbolic concentration Y₁ = λ·coth(λ) with λ = L_eng/(1.05·d)".into(),
            metric_label: format!("Y₁ = F_thread,1 / F_avg (L_eng={:.1} mm)", l_eng),
            standard_value: sopwith_handbook_factor,
            apriori_value: sopwith_apriori_factor,
            discrepancy_pct: err_b4,
            regime_insight:
                "Explains why 78% of fastener fatigue and brittle SiC boss fractures initiate at the very first engaged thread root despite adding extra thread length beyond L_eng ≈ 1.5·d.".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Document, LoadKind, Node, NodeKind, Op, ProfileVertex, Transform};

    #[test]
    fn rotated_extrude_surface_frame_and_hex_cap_washerless_and_load_case() {
        let profile = vec![
            ProfileVertex { x: 0.0, y: 0.0, bulge: 0.0 },
            ProfileVertex { x: 80.0, y: 0.0, bulge: 0.0 },
            ProfileVertex { x: 80.0, y: 8.0, bulge: 0.0 },
            ProfileVertex { x: 8.0, y: 8.0, bulge: 0.0 },
            ProfileVertex { x: 8.0, y: 60.0, bulge: 0.0 },
            ProfileVertex { x: 0.0, y: 60.0, bulge: 0.0 },
        ];
        let mut doc = Document {
            material: "al7075".into(),
            ..Default::default()
        };
        doc.root.push(Node {
            id: "bracket".into(),
            op: Op::Add,
            kind: NodeKind::Extrude {
                profile,
                height: 40.0,
            },
            transform: Transform {
                translate: [0.0, 0.0, 0.0],
                rotate: [90.0, 0.0, 0.0],
            },
            blend: 0.0,
            suppressed: false,
            material: None,
            color: None,
        });

        let bottom = resolve_host_surface_frame(&doc, Some("bracket"), Face::Bottom);
        assert!((bottom.face_center[0] - 40.0).abs() < 1e-6);
        assert!(bottom.face_center[1].abs() < 1e-6);
        assert!(bottom.face_center[2].abs() < 1e-6);
        assert!((bottom.face_width - 80.0).abs() < 1e-6);
        assert!((bottom.face_height - 40.0).abs() < 1e-6);

        let top = resolve_host_surface_frame(&doc, Some("bracket"), Face::Top);
        assert!((top.face_center[0] - 4.0).abs() < 1e-6);
        assert!(top.face_center[1].abs() < 1e-6);
        assert!((top.face_center[2] - 60.0).abs() < 1e-6);
        assert!((top.face_width - 8.0).abs() < 1e-6);
        assert!((top.face_height - 40.0).abs() < 1e-6);

        let nx = resolve_host_surface_frame(&doc, Some("bracket"), Face::Nx);
        assert!(nx.face_center[0].abs() < 1e-6);
        assert!(nx.face_center[1].abs() < 1e-6);
        assert!((nx.face_center[2] - 30.0).abs() < 1e-6);
        assert!((nx.face_width - 40.0).abs() < 1e-6);
        assert!((nx.face_height - 60.0).abs() < 1e-6);

        // Insert M6 HexCapBolt with include_washer = false and load_case = "Load 1".
        let summary = insert_fixing_into_document(
            &mut doc,
            &FixingInsertRequest {
                kind: FixingKind::HexCapBolt,
                size: "M6".into(),
                engagement_mm: 8.0,
                host_id: Some("bracket".into()),
                host_face: Some(Face::Bottom),
                placement_mode: Some(FixingPlacementMode::Center),
                dx_edge_mm: None,
                dy_edge_mm: None,
                u_offset_mm: Some(15.0),
                v_offset_mm: Some(0.0),
                flange_inset_x_mm: None,
                flange_inset_y_mm: None,
                bolt_circle_radius_mm: None,
                bolt_circle_count: None,
                bolt_circle_start_deg: None,
                custom_points_uv: Vec::new(),
                at: None,
                group_in_tree: Some(true),
                associate_with_host_group: false,
                bind_surface_lets: Some(true),
                include_washer: false,
                associated_nut: AssociatedNutKind::None,
                preload_cv: None,
                load_case: Some("Load 1".into()),
                fastener_material: "steel_10_9".into(),
                substrate_material: Some("al7075".into()),
                vibration_g: 5.0,
                payload_mass_kg: 0.25,
                id_prefix: Some("m6_bolt".into()),
            },
        );
        assert!(!summary.first_new_id.is_empty());

        let cbore = doc.find("m6_bolt_counterbore").expect("counterbore");
        if let NodeKind::Cylinder { height, .. } = &cbore.kind {
            assert!((*height - 0.4).abs() < 1e-6, "cbore should remain 0.4 mm spotface, got {height}");
        } else {
            panic!("expected cylinder cbore");
        }
        let cap_head = doc.find("m6_bolt_cap_head").expect("cap_head");
        assert!(
            cap_head.transform.translate[2] > 2.5,
            "washerless cap_head should sit proud of surface, got z={}",
            cap_head.transform.translate[2]
        );
        let preload = doc
            .loads
            .iter()
            .find(|l| matches!(l.kind, LoadKind::Preload { .. }))
            .expect("preload load");
        assert_eq!(preload.case, "Load 1");
    }
}
