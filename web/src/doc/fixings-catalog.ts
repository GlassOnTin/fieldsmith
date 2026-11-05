import type { Document, Face, Node, ProfileVertex, Vec3 } from './types';
import { allIds, findNode } from './types';
import { ensureContinuumMaterialInDoc, upsertLetParam } from './continuum';
import {
  associateNodesWithHost,
  resolveHostSurfaceFrame,
  upsertConsumerExpr,
} from './ops';

export type FixingKindId =
  | 'self_tapping_screw'
  | 'hex_head_screw'
  | 'hex_cap_bolt'
  | 'washer'
  | 'hex_nut'
  | 'nyloc_nut'
  | 'hook_clip_box';

export type SubstrateRegimeId =
  | 'plastic_boss'
  | 'metal_housing'
  | 'brittle_weibull_optics'
  | 'woven_composite';

export interface FixingCatalogueItem {
  id: FixingKindId;
  title: string;
  shortLabel: string;
  isoStandard: string;
  openStandardCode: string;
  openStandardUrl: string;
  openStandardClause: string;
  icon: string;
  summary: string;
  autoHostFeatures: string;
  slidingSurfaceMechanism: string;
}

export const FIXINGS_CATALOGUE: FixingCatalogueItem[] = [
  {
    id: 'self_tapping_screw',
    title: 'Self-Tapping Thread-Forming Screw',
    shortLabel: 'Self-Tapping Screw',
    isoStandard: 'Plastite 30° / ISO 1478',
    openStandardCode: 'NASA-STD-5020B §4.4 / MIL-HDBK-60 §3.4',
    openStandardUrl: 'https://standards.nasa.gov/standard/NASA/NASA-STD-5020',
    openStandardClause:
      'Prevailing/thread-forming torque T_form separates installation torque from net axial clamp preload F_preload.',
    icon: '🔩',
    summary:
      'Trilobular / sharp-crest thread-forming screw that cold-flows or cuts internal mating threads into an untapped pilot hole.',
    autoHostFeatures:
      'Auto-subtracts optimal pilot bore (d_pilot ≈ 0.80 d for plastic, 0.83 d for metal), spotface lead-in chamfer, thread-bite groove torus, and adds reinforcing boss collar on plastic parts.',
    slidingSurfaceMechanism:
      'Helical inclined plane (lead P/(2π)) + flank sliding friction (μ_t r_t / cos α_n) + underhead bearing friction (μ_h r_h) + plastic/elastic thread-forming torque T_form.',
  },
  {
    id: 'hex_head_screw',
    title: 'Hexagonal Head Machine Screw',
    shortLabel: 'Hex Head Screw',
    isoStandard: 'ISO 4017 / DIN 933',
    openStandardCode: 'FED-STD-H28 §2B / ASME B1.13M',
    openStandardUrl: 'https://assist.dla.mil/',
    openStandardClause:
      '60° M-profile fundamental triangle H = (√3/2)·P, tensile stress area A_s = (π/4)·(d − 0.938194·P)².',
    icon: '⬢',
    summary:
      'External 6-flat wrench-driven hexagonal head machine screw for high-torque structural clamping.',
    autoHostFeatures:
      'Auto-subtracts bearing spotface + tapped internal thread bore in the fixed object and places a 6-vertex extruded ISO hex head with preload region.',
    slidingSurfaceMechanism:
      'ISO 60° (α_n = 30°) helical thread flank sliding + flat hex under-head bearing ring sliding.',
  },
  {
    id: 'hex_cap_bolt',
    title: 'Socket Head Cap Bolt (Allen Key)',
    shortLabel: 'Hex Cap Bolt',
    isoStandard: 'ISO 4762 / DIN 912',
    openStandardCode: 'NASA-STD-5020B App. A / Sopwith (1948)',
    openStandardUrl: 'https://standards.nasa.gov/standard/NASA/NASA-STD-5020',
    openStandardClause:
      'Combined tension–torsion bolt core stress σ_vm = √(σ_ax² + 3τ_tor²) and 1st-thread load concentration Y₁ = λ·coth(λ).',
    icon: '⚙️',
    summary:
      'High-strength cylindrical cap head bolt with internal 6-sided Allen hex socket recess for compact aerospace & instrument flanges.',
    autoHostFeatures:
      'Auto-subtracts cylindrical counterbore pocket + tapped bolt bore in the fixed object and adds cap head with subtracted 6-sided Allen socket recess.',
    slidingSurfaceMechanism:
      'High-pressure annular counterbore seat sliding + ISO 60° flank sliding + VDI 2230 combined tension–torsion core stress.',
  },
  {
    id: 'washer',
    title: 'Structural Load-Spreading Washer',
    shortLabel: 'Washer',
    isoStandard: 'ISO 7089 / DIN 125A',
    openStandardCode: 'CMH-17 Vol. 3 / NASA RP-1351',
    openStandardUrl: 'https://ntrs.nasa.gov/citations/19950009349',
    openStandardClause:
      'Expands bearing frustum footprint A_bearing to suppress viscoelastic polymer creep and Tsai–Wu composite crush.',
    icon: '⭕',
    summary:
      'Hardened annular bearing washer that spreads bolt clamping preload over a larger substrate footprint to prevent polymer creep or composite crushing.',
    autoHostFeatures:
      'Auto-subtracts shallow spotface seating recess + through clearance hole in the fixed object and adds annular washer disc + preload region.',
    slidingSurfaceMechanism:
      'Increases effective bearing area A_bearing to reduce compressive creep relaxation and Tsai–Wu interlaminar peel stress.',
  },
  {
    id: 'hex_nut',
    title: 'Hexagonal Structural Nut',
    shortLabel: 'Hex Nut',
    isoStandard: 'ISO 4032 / DIN 934',
    openStandardCode: 'FED-STD-H28 §2B (Internal Thread Shear)',
    openStandardUrl: 'https://assist.dla.mil/',
    openStandardClause:
      'Internal cylindrical thread-stripping shear area A_s,int = 0.58·π·d·L_eng across captive 6-flat nut engagement.',
    icon: '⬡',
    summary:
      'Standard 6-sided hexagonal nut for through-bolted joints or captive hex nut-trap pockets in 3D-printed / machined housings.',
    autoHostFeatures:
      'Auto-subtracts captive 6-sided anti-rotation hex nut-trap pocket + through-bolt clearance bore in the fixed object.',
    slidingSurfaceMechanism:
      'Captive hex flats react installation torque while internal helical threads convert rotary motion into axial bolt tension.',
  },
  {
    id: 'nyloc_nut',
    title: 'Nyloc Prevailing-Torque Lock Nut',
    shortLabel: 'Nyloc Nut',
    isoStandard: 'ISO 10511 / DIN 985',
    openStandardCode: 'NASA-STD-5020B §4.3 (Locking Devices)',
    openStandardUrl: 'https://standards.nasa.gov/standard/NASA/NASA-STD-5020',
    openStandardClause:
      'Positive prevailing-torque locking feature resists Junker transverse slip self-loosening under random vibration.',
    icon: '🔒',
    summary:
      'Hex lock nut with domed collar housing an unthreaded PA12 nylon insert ring that radially grips the bolt threads to resist vibration loosening.',
    autoHostFeatures:
      'Auto-subtracts hex nut pocket + bolt clearance bore and adds multi-material hex body + PA12 nylon prevailing-torque insert ring.',
    slidingSurfaceMechanism:
      'Adds radial viscoelastic interference prevailing torque T_prev that raises Junker self-loosening resistance under transverse vibration.',
  },
  {
    id: 'hook_clip_box',
    title: 'Cantilever Hook-Clip Plastic Box Snap-Fit',
    shortLabel: 'Hook-Clip Box',
    isoStandard: 'VDI 2251 Snap-Fit',
    openStandardCode: 'NASA Goddard Plastics Snap-Fit Design Guide',
    openStandardUrl: 'https://ntrs.nasa.gov/',
    openStandardClause:
      'Euler–Bernoulli tapered cantilever deflection y_max = 0.67·ε_allow·L²/t with inclined ramp friction μ+tan(α)/(1−μ·tan(α)).',
    icon: '📎',
    summary:
      'Automatic tool-less cantilever hook-clip latch and mated undercut catch pocket for snap-fit instrument enclosures and plastic boxes.',
    autoHostFeatures:
      'Auto-subtracts ramped entry channel & undercut retention catch slot in the fixed box wall and adds filleted cantilever latch arm + retention hook nose.',
    slidingSurfaceMechanism:
      'Inclined lead-in ramp (angle α_ramp) deflects the cantilever beam elastically during assembly; steeper return flank (β_ret) locks against knock release.',
  },
];

export interface FixingSizeSpec {
  name: string;
  major_d: number;
  pitch: number;
  pilot_d_plastic: number;
  pilot_d_metal: number;
  clearance_d: number;
  hex_flats_s: number;
  cap_head_d: number;
  head_h: number;
  allen_s: number;
  washer_od: number;
  washer_t: number;
  nut_h: number;
  boss_od: number;
}

export const FIXING_SIZES: FixingSizeSpec[] = [
  {
    name: 'M2.5',
    major_d: 2.5,
    pitch: 0.45,
    pilot_d_plastic: 2.0,
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
  {
    name: 'M3',
    major_d: 3.0,
    pitch: 0.5,
    pilot_d_plastic: 2.4,
    pilot_d_metal: 2.5,
    clearance_d: 3.4,
    hex_flats_s: 5.5,
    cap_head_d: 5.5,
    head_h: 3.0,
    allen_s: 2.5,
    washer_od: 7.0,
    washer_t: 0.5,
    nut_h: 2.4,
    boss_od: 7.5,
  },
  {
    name: 'M4',
    major_d: 4.0,
    pitch: 0.7,
    pilot_d_plastic: 3.2,
    pilot_d_metal: 3.3,
    clearance_d: 4.5,
    hex_flats_s: 7.0,
    cap_head_d: 7.0,
    head_h: 4.0,
    allen_s: 3.0,
    washer_od: 9.0,
    washer_t: 0.8,
    nut_h: 3.2,
    boss_od: 10.0,
  },
  {
    name: 'M5',
    major_d: 5.0,
    pitch: 0.8,
    pilot_d_plastic: 4.05,
    pilot_d_metal: 4.2,
    clearance_d: 5.5,
    hex_flats_s: 8.0,
    cap_head_d: 8.5,
    head_h: 5.0,
    allen_s: 4.0,
    washer_od: 10.0,
    washer_t: 1.0,
    nut_h: 4.7,
    boss_od: 12.5,
  },
  {
    name: 'M6',
    major_d: 6.0,
    pitch: 1.0,
    pilot_d_plastic: 4.9,
    pilot_d_metal: 5.0,
    clearance_d: 6.6,
    hex_flats_s: 10.0,
    cap_head_d: 10.0,
    head_h: 6.0,
    allen_s: 5.0,
    washer_od: 12.0,
    washer_t: 1.6,
    nut_h: 5.2,
    boss_od: 15.0,
  },
  {
    name: 'M8',
    major_d: 8.0,
    pitch: 1.25,
    pilot_d_plastic: 6.6,
    pilot_d_metal: 6.8,
    clearance_d: 9.0,
    hex_flats_s: 13.0,
    cap_head_d: 13.0,
    head_h: 8.0,
    allen_s: 6.0,
    washer_od: 16.0,
    washer_t: 1.6,
    nut_h: 6.8,
    boss_od: 20.0,
  },
];

export function findFixingSize(name: string): FixingSizeSpec {
  const norm = name.trim().toUpperCase();
  return FIXING_SIZES.find((s) => s.name === norm) ?? FIXING_SIZES[2]; // default M4
}

export function classifySubstrateRegime(matId: string): SubstrateRegimeId {
  const m = matId.trim().toLowerCase();
  if (
    m === 'pla' ||
    m === 'petg' ||
    m === 'abs' ||
    m === 'pa12' ||
    m === 'nylon_pa66' ||
    m === 'pc_lexan' ||
    m === 'pom_delrin' ||
    m === 'peek_cf30' ||
    m === 'ptfe_teflon'
  ) {
    return 'plastic_boss';
  }
  if (
    m === 'cf_woven_3k' ||
    m === 'fr4_g10' ||
    m === 'cfrp' ||
    m === 'woven_composite'
  ) {
    return 'woven_composite';
  }
  if (
    m === 'sic_jwst' ||
    m === 'alumina_al2o3' ||
    m === 'nbk7_glass' ||
    m === 'soda_lime_glass' ||
    m === 'sic_refractory' ||
    m === 'ceramic'
  ) {
    return 'brittle_weibull_optics';
  }
  return 'metal_housing';
}

export function substrateRegimeLabel(regime: SubstrateRegimeId): string {
  switch (regime) {
    case 'plastic_boss':
      return 'Thermoplastic Boss (Viscoplastic Creep + Lamé Boss Split + Thread Strip)';
    case 'metal_housing':
      return 'Ductile Metal Housing (Combined Tension–Torsion Yield + Thread Shear Strip)';
    case 'brittle_weibull_optics':
      return 'Brittle / Space-Optics Substrate (Weibull Principal-Stress Fracture P_f)';
    case 'woven_composite':
      return 'Anisotropic Woven Composite (Tsai–Wu Interlaminar Peel & Bearing Delamination)';
  }
}

export interface FixingMaterialOption {
  id: string;
  label: string;
  category: string;
  e: number;
  nu: number;
  yield_strength: number;
  density: number;
  alpha: number;
  yield_z: number;
  shear_z: number;
  source: string;
}

export const FASTENER_MATERIAL_GROUPS: { group: string; options: FixingMaterialOption[] }[] = [
  {
    group: 'Carbon & Alloy Steel Fasteners (ISO 898-1)',
    options: [
      {
        id: 'steel_8_8',
        label: 'Steel Class 8.8 Medium-Carbon Q&T (σ_y = 640 MPa, ISO 898-1)',
        category: 'Carbon & Alloy Steel Fasteners (ISO 898-1)',
        e: 210000,
        nu: 0.29,
        yield_strength: 640,
        density: 7.85,
        alpha: 11.5e-6,
        yield_z: 384,
        shear_z: 369.5,
        source: 'ISO 898-1 Property Class 8.8 quenched and tempered steel fastener: E=210 GPa, Rp0.2=640 MPa, Rm=800 MPa.',
      },
      {
        id: 'steel_10_9',
        label: 'Steel Class 10.9 Alloy High-Tensile (σ_y = 900 MPa, ISO 898-1)',
        category: 'Carbon & Alloy Steel Fasteners (ISO 898-1)',
        e: 210000,
        nu: 0.29,
        yield_strength: 900,
        density: 7.85,
        alpha: 11.5e-6,
        yield_z: 540,
        shear_z: 519.6,
        source: 'ISO 898-1 Property Class 10.9 alloy steel fastener: E=210 GPa, Rp0.2=900 MPa, Rm=1040 MPa.',
      },
      {
        id: 'steel_12_9',
        label: 'Steel Class 12.9 Socket Cap CrMo Alloy (σ_y = 1080 MPa, ISO 4762)',
        category: 'Carbon & Alloy Steel Fasteners (ISO 898-1)',
        e: 210000,
        nu: 0.29,
        yield_strength: 1080,
        density: 7.85,
        alpha: 11.5e-6,
        yield_z: 648,
        shear_z: 623.5,
        source: 'ISO 898-1 Property Class 12.9 CrMo socket head cap screw: E=210 GPa, Rp0.2=1080 MPa, Rm=1220 MPa.',
      },
      {
        id: 'steel_4_8',
        label: 'Steel Class 4.8 / 4.6 Mild Carbon Steel (σ_y = 320 MPa, ISO 898-1)',
        category: 'Carbon & Alloy Steel Fasteners (ISO 898-1)',
        e: 205000,
        nu: 0.29,
        yield_strength: 320,
        density: 7.85,
        alpha: 12.0e-6,
        yield_z: 192,
        shear_z: 184.8,
        source: 'ISO 898-1 Property Class 4.8 low-carbon steel screw: E=205 GPa, ReL=320 MPa, Rm=420 MPa.',
      },
    ],
  },
  {
    group: 'Stainless Steel Fasteners (ISO 3506-1)',
    options: [
      {
        id: 'ss304_a2_70',
        label: 'A2-70 / 304 Stainless Steel (σ_y = 450 MPa, ISO 3506-1)',
        category: 'Stainless Steel Fasteners (ISO 3506-1)',
        e: 193000,
        nu: 0.29,
        yield_strength: 450,
        density: 7.93,
        alpha: 17.2e-6,
        yield_z: 270,
        shear_z: 259.8,
        source: 'ISO 3506-1 Grade A2-70 cold-worked 18/8 austenitic stainless fastener: E=193 GPa, Rp0.2=450 MPa, Rm=700 MPa.',
      },
      {
        id: 'ss316l',
        label: 'A4-80 / 316L Marine Stainless Steel (σ_y = 600 MPa, ISO 3506-1)',
        category: 'Stainless Steel Fasteners (ISO 3506-1)',
        e: 193000,
        nu: 0.29,
        yield_strength: 600,
        density: 7.99,
        alpha: 16.0e-6,
        yield_z: 360,
        shear_z: 346.4,
        source: 'ISO 3506-1 Grade A4-80 / 316L marine molybdenum austenitic stainless fastener: E=193 GPa, Rp0.2=600 MPa, Rm=800 MPa.',
      },
      {
        id: 'ss17_4ph',
        label: '17-4 PH (H900) Precipitation-Hardened Stainless (σ_y = 1170 MPa)',
        category: 'Stainless Steel Fasteners (ISO 3506-1)',
        e: 197000,
        nu: 0.27,
        yield_strength: 1170,
        density: 7.78,
        alpha: 10.8e-6,
        yield_z: 702,
        shear_z: 675.5,
        source: 'AMS 5643 17-4 PH Condition H900 aerospace stainless bolt: E=197 GPa, Rp0.2=1170 MPa.',
      },
    ],
  },
  {
    group: 'Aerospace Titanium & Nickel Superalloys',
    options: [
      {
        id: 'ti64',
        label: 'Ti-6Al-4V Grade 5 Aerospace Titanium (σ_y = 960 MPa, AMS 4928)',
        category: 'Aerospace Titanium & Nickel Superalloys',
        e: 110000,
        nu: 0.34,
        yield_strength: 960,
        density: 4.43,
        alpha: 9.0e-6,
        yield_z: 576,
        shear_z: 554.3,
        source: 'ISO 5832-3 / AMS 4928 Ti-6Al-4V Grade 5 titanium fastener: E=110 GPa, Rp0.2=960 MPa.',
      },
      {
        id: 'in718',
        label: 'Inconel 718 High-Temp Superalloy (σ_y = 980 MPa, AMS 5662)',
        category: 'Aerospace Titanium & Nickel Superalloys',
        e: 190000,
        nu: 0.29,
        yield_strength: 980,
        density: 8.19,
        alpha: 13.0e-6,
        yield_z: 588,
        shear_z: 565.8,
        source: 'AMS 5662 Inconel 718 precipitation-hardened bolt: E=190 GPa, Rp0.2=980 MPa.',
      },
      {
        id: 'a286_superalloy',
        label: 'A-286 Iron-Nickel Superalloy Bolt (σ_y = 655 MPa, AMS 5732)',
        category: 'Aerospace Titanium & Nickel Superalloys',
        e: 201000,
        nu: 0.30,
        yield_strength: 655,
        density: 7.92,
        alpha: 16.5e-6,
        yield_z: 393,
        shear_z: 378.2,
        source: 'AMS 5732 A-286 cryogenic & turbine fastener alloy: E=201 GPa, Rp0.2=655 MPa.',
      },
    ],
  },
  {
    group: 'Non-Ferrous Metals (Aluminium, Brass, Bronze, Copper)',
    options: [
      {
        id: 'al7075_t6',
        label: 'Aluminium 7075-T6 High-Strength Anodized (σ_y = 503 MPa)',
        category: 'Non-Ferrous Metals (Aluminium, Brass, Bronze, Copper)',
        e: 71700,
        nu: 0.33,
        yield_strength: 503,
        density: 2.81,
        alpha: 23.4e-6,
        yield_z: 301.8,
        shear_z: 290.4,
        source: 'AA 7075-T6 aerospace aluminium fastener: E=71.7 GPa, Rp0.2=503 MPa, Rm=572 MPa.',
      },
      {
        id: 'alsi10mg',
        label: 'Aluminium 6061-T6 / AlSi10Mg Lightweight (σ_y = 240 MPa)',
        category: 'Non-Ferrous Metals (Aluminium, Brass, Bronze, Copper)',
        e: 70000,
        nu: 0.33,
        yield_strength: 240,
        density: 2.68,
        alpha: 21.0e-6,
        yield_z: 144.0,
        shear_z: 138.6,
        source: 'AA 6061-T6 / AlSi10Mg lightweight aluminium screw: E=70 GPa, Rp0.2=240 MPa.',
      },
      {
        id: 'brass_cuZn39pb3',
        label: 'Brass CW614N / CuZn39Pb3 Electrical & Marine (σ_y = 260 MPa)',
        category: 'Non-Ferrous Metals (Aluminium, Brass, Bronze, Copper)',
        e: 97000,
        nu: 0.34,
        yield_strength: 260,
        density: 8.47,
        alpha: 20.5e-6,
        yield_z: 156.0,
        shear_z: 150.1,
        source: 'EN 12164 CW614N (CuZn39Pb3 / C36000) free-machining brass fastener: E=97 GPa, Rp0.2=260 MPa.',
      },
      {
        id: 'bronze_c65100',
        label: 'Silicon Bronze C65100 / CuSi3 Marine Fastener (σ_y = 380 MPa)',
        category: 'Non-Ferrous Metals (Aluminium, Brass, Bronze, Copper)',
        e: 105000,
        nu: 0.34,
        yield_strength: 380,
        density: 8.75,
        alpha: 18.0e-6,
        yield_z: 228.0,
        shear_z: 219.4,
        source: 'ASTM F468 C65100 Silicon Bronze cold-headed bolt: E=105 GPa, Rp0.2=380 MPa.',
      },
      {
        id: 'cucrzr',
        label: 'CuCrZr High-Conductivity Copper Alloy (σ_y = 420 MPa)',
        category: 'Non-Ferrous Metals (Aluminium, Brass, Bronze, Copper)',
        e: 128000,
        nu: 0.34,
        yield_strength: 420,
        density: 8.90,
        alpha: 17.0e-6,
        yield_z: 252.0,
        shear_z: 242.5,
        source: 'CW106C CuCr1Zr precipitation-hardened copper fastener: E=128 GPa, Rp0.2=420 MPa.',
      },
    ],
  },
  {
    group: 'Polymer & Electrical-Insulating Fasteners',
    options: [
      {
        id: 'nylon_pa66',
        label: 'PA66 Nylon 6/6 Molded Insulating Fastener (σ_y = 75 MPa)',
        category: 'Polymer & Electrical-Insulating Fasteners',
        e: 2800,
        nu: 0.39,
        yield_strength: 75,
        density: 1.14,
        alpha: 80.0e-6,
        yield_z: 45.0,
        shear_z: 43.3,
        source: 'DIN 34812 PA66 Nylon 6/6 dry-as-molded metric screw/nut: E=2.8 GPa, σ_y=75 MPa.',
      },
      {
        id: 'peek_cf30',
        label: 'PEEK Aerospace High-Temp Polymer Fastener (σ_y = 100 MPa, 250 °C)',
        category: 'Polymer & Electrical-Insulating Fasteners',
        e: 3900,
        nu: 0.38,
        yield_strength: 100,
        density: 1.32,
        alpha: 47.0e-6,
        yield_z: 60.0,
        shear_z: 57.7,
        source: 'Victrex PEEK 450G high-temperature polymer screw: E=3.9 GPa, σ_y=100 MPa.',
      },
      {
        id: 'pc_lexan',
        label: 'Polycarbonate (PC) Transparent Insulating Screw (σ_y = 62 MPa)',
        category: 'Polymer & Electrical-Insulating Fasteners',
        e: 2400,
        nu: 0.37,
        yield_strength: 62,
        density: 1.20,
        alpha: 65.0e-6,
        yield_z: 37.2,
        shear_z: 35.8,
        source: 'Lexan / Makrolon Polycarbonate metric fastener: E=2.4 GPa, σ_y=62 MPa.',
      },
      {
        id: 'pom_delrin',
        label: 'POM Acetal (Delrin) Low-Friction Fastener / Clip (σ_y = 68 MPa)',
        category: 'Polymer & Electrical-Insulating Fasteners',
        e: 3100,
        nu: 0.35,
        yield_strength: 68,
        density: 1.41,
        alpha: 85.0e-6,
        yield_z: 40.8,
        shear_z: 39.3,
        source: 'DuPont Delrin 100P POM homopolymer: E=3.1 GPa, σ_y=68 MPa.',
      },
    ],
  },
];

export const SUBSTRATE_MATERIAL_GROUPS: { group: string; options: FixingMaterialOption[] }[] = [
  {
    group: '3D-Printed & Molded Thermoplastics (Plastic Boss Regime)',
    options: [
      {
        id: 'petg',
        label: 'PETG Boss (Thermoplastic FDM — E = 1.5 GPa, σ_y = 47 MPa)',
        category: '3D-Printed & Molded Thermoplastics',
        e: 1500,
        nu: 0.38,
        yield_strength: 47,
        density: 1.27,
        alpha: 70e-6,
        yield_z: 28.2,
        shear_z: 27.1,
        source: 'FDM PETG thermoplastic boss: E=1.5 GPa, σ_y=47 MPa, interlayer yield_z=28.2 MPa.',
      },
      {
        id: 'pla',
        label: 'PLA Boss (Rigid FDM — E = 2.3 GPa, σ_y = 51 MPa)',
        category: '3D-Printed & Molded Thermoplastics',
        e: 2300,
        nu: 0.36,
        yield_strength: 51,
        density: 1.24,
        alpha: 96e-6,
        yield_z: 30.6,
        shear_z: 29.4,
        source: 'FDM PLA thermoplastic boss: E=2.3 GPa, σ_y=51 MPa.',
      },
      {
        id: 'abs',
        label: 'ABS Enclosure Boss (FDM / Injection — E = 2.2 GPa, σ_y = 40 MPa)',
        category: '3D-Printed & Molded Thermoplastics',
        e: 2200,
        nu: 0.37,
        yield_strength: 40,
        density: 1.04,
        alpha: 86e-6,
        yield_z: 24.0,
        shear_z: 23.1,
        source: 'ABS enclosure boss: E=2.2 GPa, σ_y=40 MPa.',
      },
      {
        id: 'pa12',
        label: 'PA12 Nylon Boss (SLS — E = 1.65 GPa, σ_y = 45 MPa)',
        category: '3D-Printed & Molded Thermoplastics',
        e: 1650,
        nu: 0.39,
        yield_strength: 45,
        density: 1.01,
        alpha: 109e-6,
        yield_z: 27.0,
        shear_z: 26.0,
        source: 'SLS PA12 Nylon boss: E=1.65 GPa, σ_y=45 MPa.',
      },
      {
        id: 'nylon_pa66',
        label: 'PA66 Nylon 6/6 Injection-Molded Housing (E = 2.8 GPa, σ_y = 75 MPa)',
        category: '3D-Printed & Molded Thermoplastics',
        e: 2800,
        nu: 0.39,
        yield_strength: 75,
        density: 1.14,
        alpha: 80e-6,
        yield_z: 45.0,
        shear_z: 43.3,
        source: 'PA66 Nylon 6/6 molded boss: E=2.8 GPa, σ_y=75 MPa.',
      },
      {
        id: 'pc_lexan',
        label: 'Polycarbonate (PC / Lexan) Impact Housing (E = 2.4 GPa, σ_y = 62 MPa)',
        category: '3D-Printed & Molded Thermoplastics',
        e: 2400,
        nu: 0.37,
        yield_strength: 62,
        density: 1.20,
        alpha: 65e-6,
        yield_z: 37.2,
        shear_z: 35.8,
        source: 'Polycarbonate (PC) enclosure boss: E=2.4 GPa, σ_y=62 MPa.',
      },
      {
        id: 'pom_delrin',
        label: 'POM Acetal (Delrin) Machined Housing (E = 3.1 GPa, σ_y = 68 MPa)',
        category: '3D-Printed & Molded Thermoplastics',
        e: 3100,
        nu: 0.35,
        yield_strength: 68,
        density: 1.41,
        alpha: 85e-6,
        yield_z: 40.8,
        shear_z: 39.3,
        source: 'POM Acetal (Delrin) machined housing: E=3.1 GPa, σ_y=68 MPa.',
      },
      {
        id: 'peek_cf30',
        label: 'PEEK High-Temp Polymer Housing (E = 3.9 GPa, σ_y = 100 MPa)',
        category: '3D-Printed & Molded Thermoplastics',
        e: 3900,
        nu: 0.38,
        yield_strength: 100,
        density: 1.32,
        alpha: 47e-6,
        yield_z: 60.0,
        shear_z: 57.7,
        source: 'PEEK 450G high-temperature housing: E=3.9 GPa, σ_y=100 MPa.',
      },
    ],
  },
  {
    group: 'Aluminium, Steel, Stainless, Titanium & Non-Ferrous Housings',
    options: [
      {
        id: 'alsi10mg',
        label: 'AlSi10Mg / 6061-T6 Aluminium Housing (E = 70 GPa, σ_y = 230 MPa)',
        category: 'Metal Housings',
        e: 70000,
        nu: 0.33,
        yield_strength: 230,
        density: 2.67,
        alpha: 20e-6,
        yield_z: 138,
        shear_z: 132.8,
        source: 'LPBF AlSi10Mg / AA 6061-T6 aluminium housing: E=70 GPa, Rp0.2=230 MPa.',
      },
      {
        id: 'al7075_t6',
        label: 'Aluminium 7075-T6 Aerospace Billet (E = 71.7 GPa, σ_y = 503 MPa)',
        category: 'Metal Housings',
        e: 71700,
        nu: 0.33,
        yield_strength: 503,
        density: 2.81,
        alpha: 23.4e-6,
        yield_z: 301.8,
        shear_z: 290.4,
        source: 'AA 7075-T6 aerospace aluminium plate: E=71.7 GPa, Rp0.2=503 MPa.',
      },
      {
        id: 'steel_4_8',
        label: 'Mild Structural Steel S235 / 1020 Plate (E = 205 GPa, σ_y = 320 MPa)',
        category: 'Metal Housings',
        e: 205000,
        nu: 0.29,
        yield_strength: 320,
        density: 7.85,
        alpha: 12.0e-6,
        yield_z: 192,
        shear_z: 184.8,
        source: 'AISI 1020 / EN S235 structural steel plate: E=205 GPa, Rp0.2=320 MPa.',
      },
      {
        id: 'steel_8_8',
        label: '4140 / C45 Alloy Steel Block (E = 210 GPa, σ_y = 640 MPa)',
        category: 'Metal Housings',
        e: 210000,
        nu: 0.29,
        yield_strength: 640,
        density: 7.85,
        alpha: 11.5e-6,
        yield_z: 384,
        shear_z: 369.5,
        source: 'AISI 4140 Q&T alloy steel housing: E=210 GPa, Rp0.2=640 MPa.',
      },
      {
        id: 'ss304_a2_70',
        label: '304 / A2 Stainless Steel Housing (E = 193 GPa, σ_y = 450 MPa)',
        category: 'Metal Housings',
        e: 193000,
        nu: 0.29,
        yield_strength: 450,
        density: 7.93,
        alpha: 17.2e-6,
        yield_z: 270,
        shear_z: 259.8,
        source: 'AISI 304 austenitic stainless housing: E=193 GPa, Rp0.2=450 MPa.',
      },
      {
        id: 'ss316l',
        label: '316L Stainless Steel Housing (LPBF / Wrought — E = 180 GPa, σ_y = 470 MPa)',
        category: 'Metal Housings',
        e: 180000,
        nu: 0.29,
        yield_strength: 470,
        density: 7.99,
        alpha: 15.7e-6,
        yield_z: 282,
        shear_z: 271.4,
        source: '316L stainless steel housing: E=180 GPa, Rp0.2=470 MPa.',
      },
      {
        id: 'ti64',
        label: 'Ti-6Al-4V Aerospace Titanium Housing (E = 110 GPa, σ_y = 960 MPa)',
        category: 'Metal Housings',
        e: 110000,
        nu: 0.34,
        yield_strength: 960,
        density: 4.43,
        alpha: 9.0e-6,
        yield_z: 576,
        shear_z: 554.3,
        source: 'Ti-6Al-4V Grade 5 aerospace housing: E=110 GPa, Rp0.2=960 MPa.',
      },
      {
        id: 'in718',
        label: 'Inconel 718 Nickel Superalloy Housing (E = 190 GPa, σ_y = 980 MPa)',
        category: 'Metal Housings',
        e: 190000,
        nu: 0.29,
        yield_strength: 980,
        density: 8.19,
        alpha: 13.0e-6,
        yield_z: 588,
        shear_z: 565.8,
        source: 'Inconel 718 superalloy housing: E=190 GPa, Rp0.2=980 MPa.',
      },
      {
        id: 'brass_cuZn39pb3',
        label: 'Brass CW614N / C36000 Machined Block (E = 97 GPa, σ_y = 260 MPa)',
        category: 'Metal Housings',
        e: 97000,
        nu: 0.34,
        yield_strength: 260,
        density: 8.47,
        alpha: 20.5e-6,
        yield_z: 156,
        shear_z: 150.1,
        source: 'CW614N free-machining brass housing: E=97 GPa, Rp0.2=260 MPa.',
      },
      {
        id: 'cucrzr',
        label: 'CuCrZr Copper Thermal Block (E = 128 GPa, σ_y = 420 MPa)',
        category: 'Metal Housings',
        e: 128000,
        nu: 0.34,
        yield_strength: 420,
        density: 8.90,
        alpha: 17.0e-6,
        yield_z: 252,
        shear_z: 242.5,
        source: 'CuCrZr copper alloy block: E=128 GPa, Rp0.2=420 MPa.',
      },
      {
        id: 'az91d_mg',
        label: 'Magnesium AZ91D Die-Cast Housing (E = 45 GPa, σ_y = 160 MPa)',
        category: 'Metal Housings',
        e: 45000,
        nu: 0.35,
        yield_strength: 160,
        density: 1.81,
        alpha: 26.0e-6,
        yield_z: 96.0,
        shear_z: 92.4,
        source: 'ASTM AZ91D die-cast magnesium enclosure: E=45 GPa, Rp0.2=160 MPa.',
      },
    ],
  },
  {
    group: 'Woven Composites & PCB Laminates (Tsai–Wu Delamination Regime)',
    options: [
      {
        id: 'cf_woven_3k',
        label: '3K Woven CFRP Carbon/Epoxy Laminate (E = 70 GPa, S_z = 42 MPa)',
        category: 'Woven Composites & PCB Laminates',
        e: 70000,
        nu: 0.10,
        yield_strength: 620,
        density: 1.58,
        alpha: 2.1e-6,
        yield_z: 42.0,
        shear_z: 58.0,
        source: '3K woven carbon/epoxy laminate: E=70 GPa, interlaminar peel S_z=42 MPa, ILSS=58 MPa.',
      },
      {
        id: 'fr4_g10',
        label: 'FR-4 / G10 Woven Glass-Epoxy PCB & Structural Plate (E = 24 GPa, S_z = 38 MPa)',
        category: 'Woven Composites & PCB Laminates',
        e: 24000,
        nu: 0.14,
        yield_strength: 310,
        density: 1.85,
        alpha: 14.0e-6,
        yield_z: 38.0,
        shear_z: 45.0,
        source: 'NEMA FR-4 / G10 woven E-glass epoxy laminate: E=24 GPa, S_z=38 MPa, ILSS=45 MPa.',
      },
    ],
  },
  {
    group: 'Technical Ceramics & Optical Glasses (Weibull Brittle Regime)',
    options: [
      {
        id: 'sic_jwst',
        label: 'SiC Space-Optics Sintered Silicon Carbide (E = 410 GPa, σ₀ = 340 MPa)',
        category: 'Technical Ceramics & Optical Glasses',
        e: 410000,
        nu: 0.17,
        yield_strength: 340,
        density: 3.10,
        alpha: 2.2e-6,
        yield_z: 204,
        shear_z: 196.3,
        source: 'Boostec sintered SiC space-optics ceramic: E=410 GPa, σ₀=340 MPa, m=10.5.',
      },
      {
        id: 'alumina_al2o3',
        label: 'Alumina Al₂O₃ 99.5% Technical Ceramic Insulator (E = 370 GPa, σ₀ = 280 MPa)',
        category: 'Technical Ceramics & Optical Glasses',
        e: 370000,
        nu: 0.22,
        yield_strength: 280,
        density: 3.89,
        alpha: 8.1e-6,
        yield_z: 168,
        shear_z: 161.7,
        source: '99.5% sintered Alumina (Al₂O₃) technical ceramic: E=370 GPa, flexural σ₀=280 MPa.',
      },
      {
        id: 'nbk7_glass',
        label: 'N-BK7 / Borosilicate Optical Crown Glass (E = 82 GPa, σ₀ = 70 MPa)',
        category: 'Technical Ceramics & Optical Glasses',
        e: 82000,
        nu: 0.21,
        yield_strength: 70,
        density: 2.51,
        alpha: 7.1e-6,
        yield_z: 42.0,
        shear_z: 40.4,
        source: 'Schott N-BK7 optical borosilicate glass: E=82 GPa, Weibull σ₀=70 MPa.',
      },
    ],
  },
];

const ALL_FIXING_MATERIAL_OPTIONS: FixingMaterialOption[] = [
  ...FASTENER_MATERIAL_GROUPS.flatMap((g) => g.options),
  ...SUBSTRATE_MATERIAL_GROUPS.flatMap((g) => g.options),
];

export function findFixingMaterialOption(matId: string): FixingMaterialOption | undefined {
  return ALL_FIXING_MATERIAL_OPTIONS.find((m) => m.id === matId);
}

const BUILTIN_WASM_MAT_IDS = new Set([
  'pla',
  'petg',
  'abs',
  'pa12',
  'alsi10mg',
  'ti64',
  'ss316l',
  'cucrzr',
]);

/** Ensure that any fastener or substrate material ID is registered in `draft.custom_materials`
 * when it is not one of the 8 built-in Rust WASM materials. */
export function ensureFixingMaterialInDoc(draft: Document, matId: string): void {
  if (!matId) return;
  ensureContinuumMaterialInDoc(draft, matId);
  if (BUILTIN_WASM_MAT_IDS.has(matId)) return;
  if (!draft.custom_materials) draft.custom_materials = [];
  if (draft.custom_materials.some((m) => m.id === matId)) return;
  const opt = findFixingMaterialOption(matId);
  if (opt) {
    draft.custom_materials.push({
      id: opt.id,
      name: opt.label.split(' (')[0],
      e: opt.e,
      nu: opt.nu,
      yield_strength: opt.yield_strength,
      density: opt.density,
      min_wall: 0.6,
      alpha: opt.alpha,
      yield_z: opt.yield_z,
      shear_z: opt.shear_z,
      source: opt.source,
    });
  }
}

interface MatProps {
  e: number;
  yield_strength: number;
  alpha: number;
  yield_z: number;
  shear_z: number;
}

const SUBSTRATE_PROPS: Record<string, MatProps> = {
  pla: { e: 2300, yield_strength: 51, alpha: 96e-6, yield_z: 30.6, shear_z: 29.4 },
  petg: { e: 1500, yield_strength: 47, alpha: 70e-6, yield_z: 28.2, shear_z: 27.1 },
  abs: { e: 2200, yield_strength: 40, alpha: 86e-6, yield_z: 24.0, shear_z: 23.1 },
  pa12: { e: 1650, yield_strength: 45, alpha: 109e-6, yield_z: 27.0, shear_z: 26.0 },
  nylon_pa66: { e: 2800, yield_strength: 75, alpha: 80e-6, yield_z: 45.0, shear_z: 43.3 },
  pc_lexan: { e: 2400, yield_strength: 62, alpha: 65e-6, yield_z: 37.2, shear_z: 35.8 },
  pom_delrin: { e: 3100, yield_strength: 68, alpha: 85e-6, yield_z: 40.8, shear_z: 39.3 },
  peek_cf30: { e: 3900, yield_strength: 100, alpha: 47e-6, yield_z: 60.0, shear_z: 57.7 },
  alsi10mg: { e: 70000, yield_strength: 230, alpha: 20e-6, yield_z: 138, shear_z: 132.8 },
  al7075_t6: { e: 71700, yield_strength: 503, alpha: 23.4e-6, yield_z: 301.8, shear_z: 290.4 },
  steel_4_8: { e: 205000, yield_strength: 320, alpha: 12.0e-6, yield_z: 192, shear_z: 184.8 },
  steel_1020: { e: 205000, yield_strength: 350, alpha: 12.0e-6, yield_z: 210, shear_z: 202.1 },
  steel_8_8: { e: 210000, yield_strength: 640, alpha: 11.5e-6, yield_z: 384, shear_z: 369.5 },
  steel_10_9: { e: 210000, yield_strength: 900, alpha: 11.5e-6, yield_z: 540, shear_z: 519.6 },
  steel_12_9: { e: 210000, yield_strength: 1080, alpha: 11.5e-6, yield_z: 648, shear_z: 623.5 },
  ss304_a2_70: { e: 193000, yield_strength: 450, alpha: 17.2e-6, yield_z: 270, shear_z: 259.8 },
  ss316l: { e: 193000, yield_strength: 600, alpha: 15.7e-6, yield_z: 360, shear_z: 346.4 },
  ss17_4ph: { e: 197000, yield_strength: 1170, alpha: 10.8e-6, yield_z: 702, shear_z: 675.5 },
  ti64: { e: 110000, yield_strength: 960, alpha: 9.0e-6, yield_z: 576, shear_z: 554.3 },
  in718: { e: 190000, yield_strength: 980, alpha: 13.0e-6, yield_z: 588, shear_z: 565.8 },
  a286_superalloy: { e: 201000, yield_strength: 655, alpha: 16.5e-6, yield_z: 393, shear_z: 378.2 },
  brass_cuZn39pb3: { e: 97000, yield_strength: 260, alpha: 20.5e-6, yield_z: 156, shear_z: 150.1 },
  bronze_c65100: { e: 105000, yield_strength: 380, alpha: 18.0e-6, yield_z: 228, shear_z: 219.4 },
  cucrzr: { e: 128000, yield_strength: 420, alpha: 17.0e-6, yield_z: 252, shear_z: 242.5 },
  az91d_mg: { e: 45000, yield_strength: 160, alpha: 26.0e-6, yield_z: 96, shear_z: 92.4 },
  cf_woven_3k: { e: 70000, yield_strength: 620, alpha: 2.1e-6, yield_z: 42.0, shear_z: 58.0 },
  fr4_g10: { e: 24000, yield_strength: 310, alpha: 14.0e-6, yield_z: 38.0, shear_z: 45.0 },
  sic_jwst: { e: 410000, yield_strength: 340, alpha: 2.2e-6, yield_z: 204, shear_z: 196.3 },
  alumina_al2o3: { e: 370000, yield_strength: 280, alpha: 8.1e-6, yield_z: 168, shear_z: 161.7 },
  nbk7_glass: { e: 82000, yield_strength: 70, alpha: 7.1e-6, yield_z: 42, shear_z: 40.4 },
};

export type AssociatedNutKind = 'none' | 'hex_nut' | 'nyloc_nut';

export interface FixingEnvelopeInput {
  fixingKind: FixingKindId;
  nominalDMm: number;
  pitchMm: number;
  engagementLengthMm: number;
  bossOdMm: number;
  substrateMaterial: string;
  fastenerMaterial: string;
  vibrationG: number;
  shockG: number;
  payloadMassKg: number;
  numFixings: number;
  deltaTempC: number;
  serviceHours: number;
  targetFailureProb: number;
  muThread: number;
  muHead: number;
  muJoint: number;
  /** Whether a flat bearing washer (ISO 7089 / DIN 125A) is installed under the bolt/screw head. */
  includeWasher?: boolean;
  /** Optional mated nut associated with the bolt (`hex_nut` ISO 4032 or `nyloc_nut` ISO 10511). */
  associatedNut?: AssociatedNutKind;
  weibullMOverride?: number;
  weibullSigma0OverrideMpa?: number;
}

export interface FixingHoldableForces {
  /** Axial clamping preload generated at nominal tightening torque T_rec (N). */
  axialPreloadAtRecN: number;
  /** Long-term residual clamping force after viscoplastic creep relaxation & thermal ΔT (N). */
  residualClampAfterCreepN: number;
  /** Max external axial tensile force per bolt before joint gapping / separation F_sep = F_res / (1 − Φ_K) (N). */
  maxAxialLoadBeforeGappingN: number;
  /** Max transverse shear load held by friction grip before joint slip F_slip = μ_joint · F_res (N). */
  maxTransverseFrictionSlipN: number;
  /** Mechanical pin-shear capacity of the bolt shank across the shear plane (N). */
  maxShankShearPinN: number;
  /** Axial thread pull-out / shear stripping capacity (host tapped bore or associated nut) (N). */
  maxThreadPulloutN: number;
  /** Underhead / washer bearing crush capacity on the host surface (N). */
  maxUnderheadBearingN: number;
  /** Fastener core tensile yield capacity at 0.9 σ_y (N). */
  maxBoltTensileYieldN: number;
  /** Safe external axial working tension per bolt (min of gapping and pullout with SF = 1.25) (N). */
  safeWorkingAxialLoadN: number;
  /** Safe external transverse shear load per bolt (friction grip) (N). */
  safeWorkingShearLoadN: number;
}

export interface FixingEnvelopeReport {
  substrateRegime: SubstrateRegimeId;
  substrateRegimeLabel: string;
  governingFailureMode: string;
  includeWasher: boolean;
  associatedNut: AssociatedNutKind;
  bearingAreaMm2: number;
  bearingPressureAtRecMpa: number;
  motoshTorqueArmMm: number;
  nutFactorK: number;
  threadFormingTorqueNm: number;
  minResidualClampN: number;
  minInstallPreloadN: number;
  creepRetentionFactor: number;
  thermalPreloadShiftN: number;
  minTorqueNm: number;
  maxAllowablePreloadN: number;
  maxTorqueNm: number;
  recommendedTorqueNm: number;
  recommendedPreloadN: number;
  torqueWindowRatio: number;
  peakPrincipalStressMpa: number;
  weibullFailureProbAtRec: number;
  weibullModulusM: number;
  weibullScaleSigma0Mpa: number;
  holdableForces: FixingHoldableForces;
  modeLimitsNm: {
    weibullPrincipalFractureNm: number;
    bossCreepOrHoopSplitNm: number;
    substrateThreadStripNm: number;
    fastenerCoreYieldNm: number;
    compositeInterlaminarPeelNm: number;
    underheadBearingCrushNm: number;
  };
}

export function evaluateFixingTorqueEnvelope(inp: FixingEnvelopeInput): FixingEnvelopeReport {
  const subMat = SUBSTRATE_PROPS[inp.substrateMaterial] ?? SUBSTRATE_PROPS.petg;
  const fastMat = SUBSTRATE_PROPS[inp.fastenerMaterial] ?? SUBSTRATE_PROPS.ti64;
  const regime = classifySubstrateRegime(inp.substrateMaterial);

  const d = Math.max(1.6, inp.nominalDMm);
  const p = Math.max(0.25, inp.pitchMm);
  const lEng = Math.max(1.5 * p, inp.engagementLengthMm);
  const bossOd = Math.max(d * 1.45, inp.bossOdMm);
  const nFix = Math.max(1, inp.numFixings);
  const hasWasher = Boolean(inp.includeWasher) || inp.fixingKind === 'washer';
  const nutKind: AssociatedNutKind = inp.associatedNut ?? 'none';

  // Lookup standard head / washer dimensions for this nominal diameter
  const stdSize =
    FIXING_SIZES.find((s) => Math.abs(s.major_d - d) < 0.35) ?? FIXING_SIZES[2];
  const dHole = stdSize.clearance_d;
  const dHeadOuter =
    inp.fixingKind === 'hex_head_screw'
      ? stdSize.hex_flats_s
      : Math.max(d * 1.5, stdSize.cap_head_d);
  const dWasherOuter = Math.max(dHeadOuter * 1.35, stdSize.washer_od);
  const tWasher = stdSize.washer_t;

  // Underhead bearing area & effective friction radius (with vs. without ISO 7089 washer)
  // With a washer, the bolt head turns on the hardened washer face (smooth metal-on-metal μ_head)
  // while the washer spreads the clamping load through a 45° frustum onto the substrate.
  const bearingOuterD = hasWasher ? dWasherOuter : dHeadOuter;
  const bearingAreaMm2 = Math.max(
    2.5,
    (Math.PI / 4) * (bearingOuterD * bearingOuterD - dHole * dHole),
  );
  const bareHeadAreaMm2 = Math.max(
    2.0,
    (Math.PI / 4) * (dHeadOuter * dHeadOuter - dHole * dHole),
  );
  const washerAreaRatio = bearingAreaMm2 / bareHeadAreaMm2;

  const dPitch = d - 0.64952 * p;
  const dMinor = d - 1.22687 * p;
  const aTensile = (Math.PI / 4) * Math.pow(0.5 * (dPitch + dMinor), 2);
  const rT = 0.5 * dPitch;
  // Effective underhead friction radius r_h: with a washer, the head slides on the washer face
  // at mean radius (dHeadOuter + dHole)/4 with lower metal-on-washer friction; without a washer on
  // polymer/composite, ploughing into the bare surface increases effective μ_head and contact radius.
  const rH = hasWasher
    ? 0.25 * (dHeadOuter + dHole) * 1.04
    : 0.25 * (dHeadOuter + dHole);
  const effectiveMuHead = hasWasher
    ? Math.min(inp.muHead, regime === 'plastic_boss' || regime === 'woven_composite' ? 0.145 : inp.muHead * 0.92)
    : regime === 'plastic_boss' || regime === 'woven_composite'
      ? Math.max(inp.muHead, 0.22)
      : inp.muHead;

  const alphaRad =
    inp.fixingKind === 'self_tapping_screw' && regime === 'plastic_boss' && nutKind === 'none'
      ? (15 * Math.PI) / 180
      : (30 * Math.PI) / 180;

  const leadTermMm = p / (2 * Math.PI);
  const threadFrictionArmMm = (Math.max(0.02, inp.muThread) * rT) / Math.cos(alphaRad);
  const headFrictionArmMm = Math.max(0.02, effectiveMuHead) * rH;
  const motoshArmMm = leadTermMm + threadFrictionArmMm + headFrictionArmMm;
  const nutFactorK = motoshArmMm / d;

  let threadFormingTorqueNm = 0;
  if (inp.fixingKind === 'self_tapping_screw' && nutKind === 'none') {
    const radialBite = Math.max(0.25, 0.18 * d);
    const formShearMpa = 0.42 * subMat.yield_strength;
    const formNmm = 0.085 * formShearMpa * radialBite * d * lEng * Math.sqrt(p / d);
    threadFormingTorqueNm = Math.max(0.02, formNmm * 1e-3);
  }
  if (inp.fixingKind === 'nyloc_nut' || nutKind === 'nyloc_nut') {
    threadFormingTorqueNm += Math.max(0.05, 0.018 * Math.pow(d, 1.85));
  }

  const effClampedLen = lEng + (hasWasher ? tWasher : 0);
  const kBolt = (fastMat.e * aTensile) / Math.max(2, effClampedLen);
  const aClamped = Math.max(
    2 * aTensile,
    (Math.PI / 4) * (Math.max(bossOd, bearingOuterD) ** 2 - d * d),
  );
  const kMember = (subMat.e * aClamped) / Math.max(2, effClampedLen);
  const phiK = Math.min(0.85, Math.max(0.03, kBolt / (kBolt + kMember)));

  // Washer reduces local bearing pressure σ_brg = F / A_bearing, cutting viscoplastic creep loss!
  let creepRetentionFactor = 0.98;
  if (regime === 'plastic_boss') {
    const tHr = Math.max(0.1, inp.serviceHours);
    const creepRate = hasWasher ? 0.031 : 0.048;
    creepRetentionFactor = Math.min(0.98, Math.max(0.52, 1.0 - creepRate * Math.log(1 + tHr)));
  } else if (regime === 'woven_composite') {
    const tHr = Math.max(0.1, inp.serviceHours);
    const creepRate = hasWasher ? 0.0075 : 0.012;
    creepRetentionFactor = Math.min(0.995, Math.max(0.85, 1.0 - creepRate * Math.log(1 + tHr)));
  }

  const kEq = (kBolt * kMember) / Math.max(1, kBolt + kMember);
  const thermalPreloadShiftN = (subMat.alpha - fastMat.alpha) * inp.deltaTempC * effClampedLen * kEq;

  const g0 = 9.80665;
  const fTransVib = (Math.max(0.01, inp.payloadMassKg) * Math.max(0.1, inp.vibrationG) * g0) / nFix;
  const fAxialShock = (Math.max(0.01, inp.payloadMassKg) * Math.max(0.1, inp.shockG) * g0) / nFix;

  // Nyloc prevailing-torque lock nut reduces self-loosening preload margin requirement under vibration
  const vibFactor = nutKind === 'nyloc_nut' || inp.fixingKind === 'nyloc_nut' ? 0.92 : 1.15;
  const minResidualClampN =
    (vibFactor * fTransVib) / Math.max(0.08, inp.muJoint) +
    1.25 * fAxialShock * (1 - phiK) +
    12.0 * Math.pow(d / 4, 2);

  const thermalLossN = Math.max(0, -thermalPreloadShiftN);
  const minInstallPreloadN =
    (minResidualClampN + thermalLossN) / Math.max(0.4, creepRetentionFactor);

  const minTorqueNm = minInstallPreloadN * motoshArmMm * 1e-3 + threadFormingTorqueNm;

  const defaultWeibull: Record<SubstrateRegimeId, [number, number]> = {
    plastic_boss: [8.2, subMat.yield_strength * 1.36],
    metal_housing: [16.5, subMat.yield_strength * 1.45],
    brittle_weibull_optics: [9.0, subMat.yield_strength],
    woven_composite: [11.2, subMat.yield_strength * 0.95],
  };
  const [defM, defSig0] = defaultWeibull[regime];
  const weibullM = Math.min(45, Math.max(1.5, inp.weibullMOverride ?? defM));
  const weibullSigma0 = Math.max(10, inp.weibullSigma0OverrideMpa ?? defSig0);

  const vEffRatio = Math.min(2.5, Math.max(0.1, (Math.PI * d * p * 1.5) / 100.0));
  const pfAllow = Math.min(0.5, Math.max(1e-9, inp.targetFailureProb));
  const weibullLnTerm = Math.max(1e-12, -Math.log(1 - pfAllow) / vEffRatio);
  const sigma1WeibullAllowMpa = weibullSigma0 * Math.pow(weibullLnTerm, 1 / weibullM);

  const rI = 0.5 * d;
  const rO = 0.5 * Math.max(d * 1.35, bossOd);
  const lameHoopFactor = (rO * rO + rI * rI) / Math.max(0.5, rO * rO - rI * rI);
  // When an associated nut clamps through the part (instead of wedging tapped threads in the boss),
  // radial thread-wedge hoop splitting is eliminated!
  const radialWedge = nutKind !== 'none' ? 0.08 : Math.max(0.26, Math.tan(alphaRad));
  const bossWallArea = Math.max(8.0, Math.PI * 0.25 * (bossOd * bossOd - d * d));
  const bearingAreaThread = Math.max(
    12.0,
    Math.PI * dPitch * lEng * 0.72 + 0.45 * bossWallArea + (hasWasher ? 0.55 * bearingAreaMm2 : 0),
  );
  const cSigma1PerN = (1.35 * radialWedge * lameHoopFactor + 2.4) / bearingAreaThread;

  const fMaxWeibull = sigma1WeibullAllowMpa / cSigma1PerN;
  const weibullPrincipalFractureNm = fMaxWeibull * motoshArmMm * 1e-3 + threadFormingTorqueNm;

  // Underhead surface bearing crush limit (directly proportional to bearingAreaMm2 -> boosted by washer!)
  const allowableBearingStressMpa =
    regime === 'plastic_boss'
      ? 0.82 * subMat.yield_strength
      : regime === 'woven_composite'
        ? 1.15 * subMat.yield_z
        : 1.25 * subMat.yield_strength;
  const fMaxUnderheadBearing = allowableBearingStressMpa * bearingAreaMm2;
  const underheadBearingCrushNm =
    fMaxUnderheadBearing * motoshArmMm * 1e-3 + threadFormingTorqueNm;

  const sigmaCreepAllowMpa = 0.75 * subMat.yield_strength;
  const cHoopPerN = (0.45 * radialWedge * lameHoopFactor + 0.55) / bearingAreaThread;
  const fMaxBossCreep = Math.min(fMaxUnderheadBearing * 1.15, sigmaCreepAllowMpa / cHoopPerN);
  const bossCreepOrHoopSplitNm = fMaxBossCreep * motoshArmMm * 1e-3 + threadFormingTorqueNm;

  // Thread stripping: if associatedNut !== 'none', threads engage the high-strength metal nut (plus optional host threads)
  const effectiveThreadShearStrength =
    nutKind !== 'none'
      ? 0.577 * Math.max(subMat.yield_strength, fastMat.yield_strength * 0.75)
      : 0.577 * subMat.yield_strength;
  const effectiveThreadLen =
    nutKind !== 'none' ? Math.max(lEng, stdSize.nut_h * 1.15) : lEng;
  const aShearSub = Math.PI * d * effectiveThreadLen * 0.62;
  const fMaxThreadStrip = effectiveThreadShearStrength * aShearSub;
  const substrateThreadStripNm = fMaxThreadStrip * motoshArmMm * 1e-3 + threadFormingTorqueNm;

  const dS = Math.sqrt((4 * aTensile) / Math.PI);
  const tThreadArm = leadTermMm + threadFrictionArmMm;
  const tauPerN = (16 * tThreadArm) / (Math.PI * Math.pow(dS, 3));
  const sigmaAxPerN = 1 / aTensile;
  const vmPerN = Math.sqrt(sigmaAxPerN * sigmaAxPerN + 3 * tauPerN * tauPerN);
  const fMaxFastenerYield = (0.9 * fastMat.yield_strength) / vmPerN;
  const fastenerCoreYieldNm = fMaxFastenerYield * motoshArmMm * 1e-3 + threadFormingTorqueNm;

  const aWasherBearing = bearingAreaMm2;
  const peelPerN = Math.sqrt(
    Math.pow(0.45 / (aWasherBearing * subMat.yield_z), 2) +
      Math.pow(1.0 / (bearingAreaThread * subMat.shear_z), 2),
  );
  const fMaxCompositePeel = 1.0 / Math.max(1e-6, peelPerN);
  const compositeInterlaminarPeelNm =
    fMaxCompositePeel * motoshArmMm * 1e-3 + threadFormingTorqueNm;

  let governingFailureMode = 'Weibull Principal-Stress Fracture';
  let maxTorqueNm = weibullPrincipalFractureNm;
  let maxAllowablePreloadN = fMaxWeibull;

  const pickMin = (candidates: Array<[string, number, number]>) => {
    candidates.sort((a, b) => a[1] - b[1]);
    governingFailureMode = candidates[0][0];
    maxTorqueNm = candidates[0][1];
    maxAllowablePreloadN = candidates[0][2];
  };

  if (regime === 'plastic_boss') {
    pickMin([
      [
        hasWasher
          ? 'Viscoplastic Boss Hoop Creep / Radial Split (Washer-Distributed)'
          : 'Underhead Bearing Crush & Viscoplastic Boss Creep (No Washer)',
         Math.min(bossCreepOrHoopSplitNm, underheadBearingCrushNm),
         Math.min(fMaxBossCreep, fMaxUnderheadBearing),
      ],
      [
        nutKind !== 'none'
          ? `Mated ${nutKind === 'nyloc_nut' ? 'Nyloc' : 'Hex'} Nut Thread Shear Limit`
          : 'Polymer Internal Thread Shear Stripping',
        substrateThreadStripNm,
        fMaxThreadStrip,
      ],
      ['Weibull Principal-Stress Boss Fracture', weibullPrincipalFractureNm, fMaxWeibull],
    ]);
  } else if (regime === 'metal_housing') {
    pickMin([
      ['Fastener Core Combined Tension–Torsion Yield (VDI 2230)', fastenerCoreYieldNm, fMaxFastenerYield],
      [
        nutKind !== 'none'
          ? `Mated ${nutKind === 'nyloc_nut' ? 'Nyloc' : 'Hex'} Nut Thread Shear Limit`
          : 'Internal Housing Thread Shear Stripping',
        substrateThreadStripNm,
        fMaxThreadStrip,
      ],
      ['Underhead Bearing Yield Limit', underheadBearingCrushNm, fMaxUnderheadBearing],
      ['Weibull Principal-Stress Root Fracture', weibullPrincipalFractureNm, fMaxWeibull],
    ]);
  } else if (regime === 'brittle_weibull_optics') {
    pickMin([
      ['Weibull Principal-Stress Brittle Fracture Statistics', weibullPrincipalFractureNm, fMaxWeibull],
      ['Fastener Core Combined Tension–Torsion Yield (VDI 2230)', fastenerCoreYieldNm, fMaxFastenerYield],
    ]);
  } else if (regime === 'woven_composite') {
    pickMin([
      [
        hasWasher
          ? 'Tsai–Wu Interlaminar Peel & Washer-Spread Bearing'
          : 'Tsai–Wu Underhead Bearing Crush & Interlaminar Delamination (No Washer)',
        Math.min(compositeInterlaminarPeelNm, underheadBearingCrushNm),
        Math.min(fMaxCompositePeel, fMaxUnderheadBearing),
      ],
      ['Weibull Principal-Stress Matrix Micro-Cracking', weibullPrincipalFractureNm, fMaxWeibull],
    ]);
  }

  const recommendedTorqueNm =
    maxTorqueNm > minTorqueNm
      ? minTorqueNm + 0.55 * (maxTorqueNm - minTorqueNm)
      : 0.5 * (minTorqueNm + maxTorqueNm);
  const recommendedPreloadN =
    (Math.max(0.01, recommendedTorqueNm - threadFormingTorqueNm) * 1e3) / motoshArmMm;

  const bearingPressureAtRecMpa = recommendedPreloadN / Math.max(1.0, bearingAreaMm2);
  const peakPrincipalStressMpa = (recommendedPreloadN * cSigma1PerN) / Math.pow(washerAreaRatio, 0.18);
  const weibullFailureProbAtRec =
    1 - Math.exp(-vEffRatio * Math.pow(peakPrincipalStressMpa / weibullSigma0, weibullM));

  // Compute explicit Holdable Forces for the bolt/joint (VDI 2230 load-introduction factor n ≈ 0.35)
  const residualClampAfterCreepN = Math.max(
    0,
    recommendedPreloadN * creepRetentionFactor + Math.min(0, thermalPreloadShiftN),
  );
  const phiEffective = Math.min(0.32, 0.35 * phiK);
  const maxAxialLoadBeforeGappingN =
    residualClampAfterCreepN / Math.max(0.68, 1.0 - phiEffective);
  const maxTransverseFrictionSlipN =
    residualClampAfterCreepN * Math.max(0.08, inp.muJoint);
  const maxShankShearPinN = 0.6 * (0.577 * fastMat.yield_strength) * aTensile;
  const maxBoltTensileYieldN = 0.9 * fastMat.yield_strength * aTensile;
  const maxThreadPulloutN = fMaxThreadStrip;
  const maxUnderheadBearingN = fMaxUnderheadBearing;
  const safeWorkingAxialLoadN = Math.min(
    maxAxialLoadBeforeGappingN,
    maxThreadPulloutN / 1.5,
    maxUnderheadBearingN / 1.35,
    maxBoltTensileYieldN / 1.5,
  );
  const safeWorkingShearLoadN = maxTransverseFrictionSlipN / 1.15;

  return {
    substrateRegime: regime,
    substrateRegimeLabel: substrateRegimeLabel(regime),
    governingFailureMode,
    includeWasher: hasWasher,
    associatedNut: nutKind,
    bearingAreaMm2,
    bearingPressureAtRecMpa,
    motoshTorqueArmMm: motoshArmMm,
    nutFactorK,
    threadFormingTorqueNm,
    minResidualClampN,
    minInstallPreloadN,
    creepRetentionFactor,
    thermalPreloadShiftN,
    minTorqueNm,
    maxAllowablePreloadN,
    maxTorqueNm,
    recommendedTorqueNm,
    recommendedPreloadN,
    torqueWindowRatio: maxTorqueNm / Math.max(1e-6, minTorqueNm),
    peakPrincipalStressMpa,
    weibullFailureProbAtRec,
    weibullModulusM: weibullM,
    weibullScaleSigma0Mpa: weibullSigma0,
    holdableForces: {
      axialPreloadAtRecN: recommendedPreloadN,
      residualClampAfterCreepN,
      maxAxialLoadBeforeGappingN,
      maxTransverseFrictionSlipN,
      maxShankShearPinN,
      maxThreadPulloutN,
      maxUnderheadBearingN,
      maxBoltTensileYieldN,
      safeWorkingAxialLoadN,
      safeWorkingShearLoadN,
    },
    modeLimitsNm: {
      weibullPrincipalFractureNm,
      bossCreepOrHoopSplitNm,
      substrateThreadStripNm,
      fastenerCoreYieldNm,
      compositeInterlaminarPeelNm,
      underheadBearingCrushNm,
    },
  };
}

// ============================================================================
// Empirical Failure Probability Spreadsheet Fitter (Weibull & Log-Normal)
// ============================================================================

export interface FailureSampleRow {
  id: string;
  torqueNm: number;
  principalStressMpa: number;
  /** Optional explicit cumulative failure probability in (0, 1). If null/undefined, uses Bernard median rank (i - 0.3)/(N + 0.4). */
  failureProb: number | null;
  modeNote: string;
}

export interface FailureDistributionFit {
  weibullModulusM: number;
  weibullScaleSigma0Mpa: number;
  weibullScaleT0Nm: number;
  weibullR2: number;
  lognormalMuLn: number;
  lognormalSigmaLn: number;
  allowableStressAtTargetPfMpa: number;
  allowableTorqueAtTargetPfNm: number;
  points: Array<{
    id: string;
    torqueNm: number;
    stressMpa: number;
    pf: number;
    lnStress: number;
    lnTorque: number;
    weibullY: number;
    modeNote: string;
  }>;
}

export function fitFailureDistribution(
  samples: FailureSampleRow[],
  targetPf: number,
): FailureDistributionFit {
  const valid = samples
    .filter((s) => Number.isFinite(s.principalStressMpa) && s.principalStressMpa > 0.1 && s.torqueNm > 0.01)
    .slice()
    .sort((a, b) => a.principalStressMpa - b.principalStressMpa);

  const n = valid.length;
  if (n < 2) {
    return {
      weibullModulusM: 7.5,
      weibullScaleSigma0Mpa: 60.0,
      weibullScaleT0Nm: 1.8,
      weibullR2: 1.0,
      lognormalMuLn: Math.log(60),
      lognormalSigmaLn: 0.15,
      allowableStressAtTargetPfMpa: 35.0,
      allowableTorqueAtTargetPfNm: 1.0,
      points: [],
    };
  }

  const points = valid.map((row, idx) => {
    const medianRank = (idx + 1 - 0.3) / (n + 0.4);
    const pf = Math.min(1 - 1e-6, Math.max(1e-6, row.failureProb ?? medianRank));
    const lnStress = Math.log(row.principalStressMpa);
    const lnTorque = Math.log(row.torqueNm);
    const weibullY = Math.log(-Math.log(1 - pf));
    return {
      id: row.id,
      torqueNm: row.torqueNm,
      stressMpa: row.principalStressMpa,
      pf,
      lnStress,
      lnTorque,
      weibullY,
      modeNote: row.modeNote,
    };
  });

  const xMean = points.reduce((acc, p) => acc + p.lnStress, 0) / n;
  const xtMean = points.reduce((acc, p) => acc + p.lnTorque, 0) / n;
  const yMean = points.reduce((acc, p) => acc + p.weibullY, 0) / n;

  let sxx = 0;
  let sxy = 0;
  let syy = 0;
  let sxtx = 0;
  let sxty = 0;

  for (const pt of points) {
    const dx = pt.lnStress - xMean;
    const dxt = pt.lnTorque - xtMean;
    const dy = pt.weibullY - yMean;
    sxx += dx * dx;
    sxy += dx * dy;
    syy += dy * dy;
    sxtx += dxt * dxt;
    sxty += dxt * dy;
  }

  const m = Math.min(60, Math.max(1.2, sxy / Math.max(1e-12, sxx)));
  const intercept = yMean - m * xMean;
  const sigma0 = Math.exp(-intercept / m);

  const mt = Math.min(60, Math.max(1.2, sxty / Math.max(1e-12, sxtx)));
  const interceptT = yMean - mt * xtMean;
  const t0 = Math.exp(-interceptT / mt);

  const r2 =
    syy > 1e-12 && sxx > 1e-12 ? Math.min(1, Math.max(0, (sxy * sxy) / (sxx * syy))) : 1.0;
  const sigmaLn = Math.sqrt(sxx / Math.max(1, n - 1));

  const pfT = Math.min(0.5, Math.max(1e-9, targetPf));
  const weibullFactor = Math.pow(-Math.log(1 - pfT), 1 / m);
  const allowableStress = sigma0 * weibullFactor;
  const allowableTorque = t0 * Math.pow(-Math.log(1 - pfT), 1 / mt);

  return {
    weibullModulusM: m,
    weibullScaleSigma0Mpa: sigma0,
    weibullScaleT0Nm: t0,
    weibullR2: r2,
    lognormalMuLn: xMean,
    lognormalSigmaLn: sigmaLn,
    allowableStressAtTargetPfMpa: allowableStress,
    allowableTorqueAtTargetPfNm: allowableTorque,
    points,
  };
}

export const PLASTIC_BOSS_COUPONS: FailureSampleRow[] = [
  { id: 'PETG-1', torqueNm: 1.52, principalStressMpa: 46.8, failureProb: 0.08, modeNote: 'Boss hoop micro-craze' },
  { id: 'PETG-2', torqueNm: 1.68, principalStressMpa: 51.5, failureProb: 0.19, modeNote: 'Creep relaxation > 30%' },
  { id: 'PETG-3', torqueNm: 1.79, principalStressMpa: 54.9, failureProb: 0.31, modeNote: 'First-thread root tear' },
  { id: 'PETG-4', torqueNm: 1.88, principalStressMpa: 57.6, failureProb: 0.44, modeNote: 'Boss longitudinal split' },
  { id: 'PETG-5', torqueNm: 1.96, principalStressMpa: 60.1, failureProb: 0.56, modeNote: 'Boss longitudinal split' },
  { id: 'PETG-6', torqueNm: 2.05, principalStressMpa: 62.9, failureProb: 0.69, modeNote: 'Internal thread strip' },
  { id: 'PETG-7', torqueNm: 2.15, principalStressMpa: 65.8, failureProb: 0.81, modeNote: 'Boss burst fracture' },
  { id: 'PETG-8', torqueNm: 2.28, principalStressMpa: 69.9, failureProb: 0.92, modeNote: 'Complete thread shear-out' },
];

export const METAL_HOUSING_COUPONS: FailureSampleRow[] = [
  { id: 'AL-1', torqueNm: 4.15, principalStressMpa: 268.0, failureProb: 0.08, modeNote: 'First thread root yield' },
  { id: 'AL-2', torqueNm: 4.35, principalStressMpa: 281.0, failureProb: 0.19, modeNote: 'AlSi10Mg flank galling' },
  { id: 'AL-3', torqueNm: 4.48, principalStressMpa: 289.5, failureProb: 0.31, modeNote: 'Internal thread shear onset' },
  { id: 'AL-4', torqueNm: 4.59, principalStressMpa: 296.8, failureProb: 0.44, modeNote: 'Internal thread strip' },
  { id: 'AL-5', torqueNm: 4.69, principalStressMpa: 303.2, failureProb: 0.56, modeNote: 'Combined torsion–tension yield' },
  { id: 'AL-6', torqueNm: 4.79, principalStressMpa: 309.5, failureProb: 0.69, modeNote: 'Internal thread strip' },
  { id: 'AL-7', torqueNm: 4.91, principalStressMpa: 317.4, failureProb: 0.81, modeNote: 'Bolt necking + thread shear' },
  { id: 'AL-8', torqueNm: 5.08, principalStressMpa: 328.2, failureProb: 0.92, modeNote: 'Torsional bolt fracture' },
];

export const JWST_CRYO_OPTICS_COUPONS: FailureSampleRow[] = [
  { id: 'SIC-1', torqueNm: 4.65, principalStressMpa: 274.0, failureProb: 0.08, modeNote: 'Sub-surface flaw initiation (38 K)' },
  { id: 'SIC-2', torqueNm: 5.02, principalStressMpa: 296.0, failureProb: 0.19, modeNote: 'Weibull cone crack onset' },
  { id: 'SIC-3', torqueNm: 5.28, principalStressMpa: 311.5, failureProb: 0.31, modeNote: 'Radial thread-root cleavage' },
  { id: 'SIC-4', torqueNm: 5.49, principalStressMpa: 324.0, failureProb: 0.44, modeNote: 'Brittle flange spallation' },
  { id: 'SIC-5', torqueNm: 5.68, principalStressMpa: 335.2, failureProb: 0.56, modeNote: 'Weibull principal stress fracture' },
  { id: 'SIC-6', torqueNm: 5.88, principalStressMpa: 347.0, failureProb: 0.69, modeNote: 'Catastrophic boss cleavage' },
  { id: 'SIC-7', torqueNm: 6.12, principalStressMpa: 361.0, failureProb: 0.81, modeNote: 'Fastener + SiC seat shatter' },
  { id: 'SIC-8', torqueNm: 6.44, principalStressMpa: 380.0, failureProb: 0.92, modeNote: 'Biaxial hoop fracture' },
];

export const CFRP_PEEL_COUPONS: FailureSampleRow[] = [
  { id: 'CFRP-1', torqueNm: 1.95, principalStressMpa: 44.2, failureProb: 0.08, modeNote: '1st-ply resin matrix micro-crack' },
  { id: 'CFRP-2', torqueNm: 2.12, principalStressMpa: 48.0, failureProb: 0.19, modeNote: 'Washer perimeter Z-peel onset' },
  { id: 'CFRP-3', torqueNm: 2.24, principalStressMpa: 50.8, failureProb: 0.31, modeNote: 'Interlaminar Mode-II shear slip' },
  { id: 'CFRP-4', torqueNm: 2.34, principalStressMpa: 53.1, failureProb: 0.44, modeNote: 'Tsai–Wu peel delamination' },
  { id: 'CFRP-5', torqueNm: 2.43, principalStressMpa: 55.2, failureProb: 0.56, modeNote: 'Circumferential ply ring split' },
  { id: 'CFRP-6', torqueNm: 2.52, principalStressMpa: 57.3, failureProb: 0.69, modeNote: 'Countersink bearing crush' },
  { id: 'CFRP-7', torqueNm: 2.64, principalStressMpa: 59.9, failureProb: 0.81, modeNote: 'Multi-ply delamination growth' },
  { id: 'CFRP-8', torqueNm: 2.79, principalStressMpa: 63.4, failureProb: 0.92, modeNote: 'Through-thickness washer pull-in' },
];

export interface SnapFitMechanicsReport {
  beamLengthMm: number;
  beamWidthMm: number;
  beamThicknessMm: number;
  undercutDepthMm: number;
  insertionForcePushN: number;
  retentionForcePullN: number;
  rootFlexuralStrainPct: number;
  allowableStrainPct: number;
  strainSafetyFactor: number;
}

export function evaluateSnapFitMechanics(
  nominalDMm: number,
  engagementLengthMm: number,
  substrateMaterial: string,
  muJoint = 0.22,
): SnapFitMechanicsReport {
  const subMat = SUBSTRATE_PROPS[substrateMaterial] ?? SUBSTRATE_PROPS.petg;
  const beamW = Math.max(5.0, nominalDMm * 1.6);
  const beamT = Math.max(1.6, nominalDMm * 0.45);
  const beamL = Math.max(10.0, engagementLengthMm);
  const undercutY = Math.max(0.75, beamT * 0.45);

  // VDI 2251 tapered/filleted cantilever beam stiffness k_arm = E * b * t^3 / (4 * L^3)
  const eMpa = subMat.e;
  const kArmNPerMm = (eMpa * beamW * Math.pow(beamT, 3)) / (4.0 * Math.pow(beamL, 3));
  const fDeflectN = kArmNPerMm * undercutY;

  // Entry ramp α = 30°, retention return flank β = 65°
  const alphaEntry = (30 * Math.PI) / 180;
  const betaRet = (65 * Math.PI) / 180;
  const mu = Math.min(0.45, Math.max(0.08, muJoint));

  const wedgePush = (Math.tan(alphaEntry) + mu) / Math.max(0.15, 1 - mu * Math.tan(alphaEntry));
  const wedgePull = (Math.tan(betaRet) + mu) / Math.max(0.15, 1 - mu * Math.tan(betaRet));

  const insertionForcePushN = fDeflectN * wedgePush;
  const retentionForcePullN = fDeflectN * wedgePull;

  // Euler–Bernoulli root flexural strain ε_root = 1.5 * t * y_max / L^2 (with 0.78 taper factor)
  const rootFlexuralStrain = (1.15 * beamT * undercutY) / (beamL * beamL);
  const allowableStrain =
    substrateMaterial === 'pa12'
      ? 0.032
      : substrateMaterial === 'petg' || substrateMaterial === 'abs' || substrateMaterial === 'pla'
        ? 0.022
        : 0.006;

  return {
    beamLengthMm: beamL,
    beamWidthMm: beamW,
    beamThicknessMm: beamT,
    undercutDepthMm: undercutY,
    insertionForcePushN,
    retentionForcePullN,
    rootFlexuralStrainPct: rootFlexuralStrain * 100,
    allowableStrainPct: allowableStrain * 100,
    strainSafetyFactor: allowableStrain / Math.max(1e-6, rootFlexuralStrain),
  };
}

// ============================================================================
// Automatic Host Drilling/Tapping & 3D Fixing Hardware Insertion on Document
// ============================================================================

export function hexProfile(acrossFlatsS: number): ProfileVertex[] {
  const r = acrossFlatsS / Math.sqrt(3);
  const pts: ProfileVertex[] = [];
  for (let k = 0; k < 6; k++) {
    const angle = (k * Math.PI) / 3;
    pts.push({
      x: Math.round(r * Math.cos(angle) * 1000) / 1000,
      y: Math.round(r * Math.sin(angle) * 1000) / 1000,
      bulge: 0,
    });
  }
  return pts;
}

export type FixingPlacementMode =
  | 'edge'
  | 'center'
  | 'flange_4_corners'
  | 'bolt_circle'
  | 'sketch_points';

export interface FixingInsertSpec {
  kind: FixingKindId;
  size: string;
  engagementMm: number;
  hostId?: string;
  hostFace?: Face;
  placementMode?: FixingPlacementMode;
  /** Distance from the `-U` edge of the host face (mm) when `placementMode === 'edge'`. */
  dxEdgeMm?: number;
  /** Distance from the `-V` edge of the host face (mm) when `placementMode === 'edge'`. */
  dyEdgeMm?: number;
  /** Offset from the host face centre along U (mm) when `placementMode === 'center'`. */
  uOffsetMm?: number;
  /** Offset from the host face centre along V (mm) when `placementMode === 'center'`. */
  vOffsetMm?: number;
  /** Corner inset along U/V from face edges (mm) when `placementMode === 'flange_4_corners'`. */
  flangeInsetXMm?: number;
  flangeInsetYMm?: number;
  /** Pitch-circle radius (mm), hole count, and start angle (deg) when `placementMode === 'bolt_circle'`. */
  boltCircleRadiusMm?: number;
  boltCircleCount?: number;
  boltCircleStartDeg?: number;
  /** Explicit 2D `(u, v)` coordinates on the host face (e.g. from solved 2D Sketch circles/points). */
  customPointsUV?: { u: number; v: number }[];
  /** Organize the fixing into collapsible `<prefix>_bore` (−) and `<prefix>` (+) sub-assembly groups. */
  groupInTree?: boolean;
  /** Associate the fixing groups inside the host part's `<host>_assy` group in the Object Tree. */
  associateWithHostGroup?: boolean;
  /** Bind the surface placement dimensions (`_dx`/`_dy` or `_u`/`_v`) to parametric `let` variables. */
  bindSurfaceLets?: boolean;
  /** Include an ISO 7089 / DIN 125A bearing washer under the bolt/screw head. */
  includeWasher?: boolean;
  /** Associate a mated Hex Nut (ISO 4032) or Nyloc Lock Nut (ISO 10511) with the bolt/screw. */
  associatedNut?: AssociatedNutKind;
  fastenerMaterial: string;
  substrateMaterial?: string;
  vibrationG: number;
  payloadMassKg: number;
}

export function insertFixingIntoDocument(
  draft: Document,
  spec: FixingInsertSpec,
): { firstNewId: string; boreGroupId: string; createdGroupIds: string[]; envelope: FixingEnvelopeReport } {
  const sizeSpec = findFixingSize(spec.size);
  const subMat = spec.substrateMaterial ?? draft.material ?? 'petg';
  const regime = classifySubstrateRegime(subMat);

  if (spec.substrateMaterial && draft.material !== spec.substrateMaterial) {
    ensureFixingMaterialInDoc(draft, spec.substrateMaterial);
  }
  ensureFixingMaterialInDoc(draft, spec.fastenerMaterial);

  if (draft.root.length === 0) {
    const hostH = Math.max(10, spec.engagementMm + 4);
    draft.root.push({
      id: 'host_housing',
      op: 'add',
      blend: 0,
      suppressed: false,
      transform: { translate: [0, 0, 0], rotate: [0, 0, 0] },
      kind: { type: 'box', size: [32, 24, hostH], round: 1 },
    });
    draft.regions.push({
      id: 'housing_mount',
      selectors: [{ shape: 'host_housing', face: 'nz' }],
    });
    draft.supports.push({
      id: 'mount_fix',
      region: 'housing_mount',
      fixed: [true, true, true],
    });
  }

  const targetNode: Node | undefined =
    (spec.hostId ? findNode(draft, spec.hostId) ?? undefined : undefined) ??
    draft.root.find((n) => n.op === 'add');

  const hostFace: Face = spec.hostFace ?? 'top';
  const frame = resolveHostSurfaceFrame(draft, targetNode?.id ?? '', hostFace);
  const mode: FixingPlacementMode = spec.placementMode ?? 'edge';

  // Resolve target (u, v) points on the chosen host surface
  let pointsUV: { u: number; v: number; dx: number; dy: number }[] = [];
  if (mode === 'sketch_points' && spec.customPointsUV && spec.customPointsUV.length > 0) {
    pointsUV = spec.customPointsUV.map((p) => ({
      u: p.u,
      v: p.v,
      dx: 0.5 * frame.faceWidth + p.u,
      dy: 0.5 * frame.faceHeight + p.v,
    }));
  } else if (mode === 'flange_4_corners') {
    const inX = Math.max(
      sizeSpec.washer_od * 0.6,
      Math.min(frame.faceWidth * 0.45, spec.flangeInsetXMm ?? Math.max(5, frame.faceWidth * 0.18)),
    );
    const inY = Math.max(
      sizeSpec.washer_od * 0.6,
      Math.min(frame.faceHeight * 0.45, spec.flangeInsetYMm ?? Math.max(5, frame.faceHeight * 0.18)),
    );
    const uSpan = Math.max(1, 0.5 * frame.faceWidth - inX);
    const vSpan = Math.max(1, 0.5 * frame.faceHeight - inY);
    const corners: [number, number][] = [
      [-uSpan, -vSpan],
      [uSpan, -vSpan],
      [uSpan, vSpan],
      [-uSpan, vSpan],
    ];
    pointsUV = corners.map(([u, v]) => ({
      u,
      v,
      dx: 0.5 * frame.faceWidth + u,
      dy: 0.5 * frame.faceHeight + v,
    }));
  } else if (mode === 'bolt_circle') {
    const nBolts = Math.max(2, Math.min(16, Math.round(spec.boltCircleCount ?? 4)));
    const rPcd = Math.max(
      sizeSpec.washer_od,
      spec.boltCircleRadiusMm ?? Math.min(frame.faceWidth, frame.faceHeight) * 0.32,
    );
    const startRad = ((spec.boltCircleStartDeg ?? 0) * Math.PI) / 180;
    for (let k = 0; k < nBolts; k++) {
      const ang = startRad + (2 * Math.PI * k) / nBolts;
      const u = Math.round(rPcd * Math.cos(ang) * 1000) / 1000;
      const v = Math.round(rPcd * Math.sin(ang) * 1000) / 1000;
      pointsUV.push({
        u,
        v,
        dx: 0.5 * frame.faceWidth + u,
        dy: 0.5 * frame.faceHeight + v,
      });
    }
  } else if (mode === 'center') {
    const u = spec.uOffsetMm ?? 0;
    const v = spec.vOffsetMm ?? 0;
    pointsUV = [
      {
        u,
        v,
        dx: 0.5 * frame.faceWidth + u,
        dy: 0.5 * frame.faceHeight + v,
      },
    ];
  } else {
    // 'edge' mode (default: if dxEdgeMm / dyEdgeMm omitted, centre of face = W/2, H/2)
    const dx = spec.dxEdgeMm ?? 0.5 * frame.faceWidth;
    const dy = spec.dyEdgeMm ?? 0.5 * frame.faceHeight;
    const u = -0.5 * frame.faceWidth + dx;
    const v = -0.5 * frame.faceHeight + dy;
    pointsUV = [{ u, v, dx, dy }];
  }

  const takenIds = allIds(draft);
  let idx = 1;
  let basePrefix = spec.kind as string;
  if (takenIds.has(basePrefix) || takenIds.has(`${basePrefix}_bore`) || takenIds.has(`${basePrefix}_head`)) {
    while (
      takenIds.has(`${spec.kind}_${idx}`) ||
      takenIds.has(`${spec.kind}_${idx}_bore`)
    ) {
      idx++;
    }
    basePrefix = `${spec.kind}_${idx}`;
  }

  const isBoltOrScrew =
    spec.kind === 'self_tapping_screw' ||
    spec.kind === 'hex_head_screw' ||
    spec.kind === 'hex_cap_bolt';
  const hasWasher = Boolean(spec.includeWasher) && isBoltOrScrew;
  const nutKind: AssociatedNutKind = isBoltOrScrew ? (spec.associatedNut ?? 'none') : 'none';

  const envelope = evaluateFixingTorqueEnvelope({
    fixingKind: spec.kind,
    nominalDMm: sizeSpec.major_d,
    pitchMm: sizeSpec.pitch,
    engagementLengthMm: spec.engagementMm,
    bossOdMm: sizeSpec.boss_od,
    substrateMaterial: subMat,
    fastenerMaterial: spec.fastenerMaterial,
    vibrationG: spec.vibrationG,
    shockG: spec.vibrationG * 2.5,
    payloadMassKg: spec.payloadMassKg,
    numFixings: pointsUV.length,
    deltaTempC: 0,
    serviceHours: 1000,
    targetFailureProb: 1e-3,
    muThread: regime === 'plastic_boss' ? 0.18 : 0.14,
    muHead: regime === 'plastic_boss' ? 0.2 : 0.15,
    muJoint: 0.25,
    includeWasher: hasWasher,
    associatedNut: nutKind,
  });

  const pilotD = regime === 'plastic_boss' ? sizeSpec.pilot_d_plastic : sizeSpec.pilot_d_metal;
  const pilotR = Math.round(pilotD * 0.5 * 1000) / 1000;
  const majorR = sizeSpec.major_d * 0.5;
  const clearR = sizeSpec.clearance_d * 0.5;
  const engH = Math.max(3, spec.engagementMm);
  const washerShiftZ = hasWasher ? sizeSpec.washer_t : 0;

  upsertLetParam(draft, `${basePrefix}_d`, sizeSpec.major_d);
  upsertLetParam(draft, `${basePrefix}_pitch`, sizeSpec.pitch);
  upsertLetParam(draft, `${basePrefix}_pilot_r`, pilotR);
  upsertLetParam(draft, `${basePrefix}_torque_nm`, Number(envelope.recommendedTorqueNm.toFixed(3)));
  upsertLetParam(draft, `${basePrefix}_preload_n`, Number(envelope.recommendedPreloadN.toFixed(1)));
  upsertLetParam(
    draft,
    `${basePrefix}_f_sep_n`,
    Math.round(envelope.holdableForces.maxAxialLoadBeforeGappingN),
  );
  upsertLetParam(
    draft,
    `${basePrefix}_f_slip_n`,
    Math.round(envelope.holdableForces.maxTransverseFrictionSlipN),
  );

  const useGroup = spec.groupInTree !== false;
  const bindLets = spec.bindSurfaceLets !== false && pointsUV.length === 1;
  const createdNodes: Node[] = [];
  const createdGroupIds: string[] = [];
  let firstNewId = '';
  let firstBoreGroupId = '';

  const buildOneInstance = (instPrefix: string, ptUV: { u: number; v: number; dx: number; dy: number }) => {
    const surfacePt: Vec3 = frame.pointFromUV(ptUV.u, ptUV.v, 0);
    const rot: Vec3 = [...frame.rotateDeg];
    // When grouped, children sit at local [0, 0, z_local] inside the group at `surfacePt`.
    const [lx, ly, lz]: Vec3 = useGroup ? [0, 0, 0] : surfacePt;
    const childRot: Vec3 = useGroup ? [0, 0, 0] : rot;
    // Inside a subtract group (`<instPrefix>_bore`), child bores use `op: 'add'` so their union is subtracted!
    const boreChildOp = useGroup ? 'add' : 'subtract';

    const boreKids: Node[] = [];
    const hwKids: Node[] = [];

    const addClampLoad = (seatShapeId: string, boreShapeId: string, headShapeId: string) => {
      const regId = `${instPrefix}_seat`;
      draft.regions.push({
        id: regId,
        selectors: [
          { shape: headShapeId, face: 'top' },
          { shape: seatShapeId, face: 'bottom' },
          { shape: boreShapeId, face: 'side' },
        ],
      });
      draft.loads.push({
        id: `${instPrefix}_preload`,
        region: regId,
        case: 'default',
        kind: {
          type: 'preload',
          newtons: Math.round(envelope.recommendedPreloadN * 10) / 10,
        },
      });
    };

    if (spec.kind === 'self_tapping_screw') {
      if (regime === 'plastic_boss') {
        hwKids.push({
          id: `${instPrefix}_boss`,
          op: 'add',
          blend: 0.4,
          suppressed: false,
          transform: { translate: [lx, ly, lz - engH * 0.5], rotate: childRot },
          kind: { type: 'cylinder', radius: sizeSpec.boss_od * 0.5, height: engH, round: 0.3 },
        });
      }
      const seatId = `${instPrefix}_seat_recess`;
      boreKids.push({
        id: seatId,
        op: boreChildOp,
        blend: 0,
        suppressed: false,
        transform: { translate: [lx, ly, lz - 0.3], rotate: childRot },
        kind: { type: 'cylinder', radius: sizeSpec.washer_od * 0.5, height: 0.8, round: 0 },
      });
      const tapBoreId = `${instPrefix}_tap_bore`;
      boreKids.push({
        id: tapBoreId,
        op: boreChildOp,
        blend: 0,
        suppressed: false,
        transform: { translate: [lx, ly, lz - engH * 0.5], rotate: childRot },
        kind: { type: 'cylinder', radius: pilotR, height: engH + 1.0, round: 0 },
      });
      boreKids.push({
        id: `${instPrefix}_thread_bite`,
        op: boreChildOp,
        blend: 0,
        suppressed: false,
        transform: { translate: [lx, ly, lz - engH * 0.45], rotate: childRot },
        kind: {
          type: 'torus',
          major: Math.round((pilotR + majorR) * 0.5 * 100) / 100,
          minor: Math.max(0.22, Math.round((majorR - pilotR) * 100) / 100),
        },
      });
      const headId = `${instPrefix}_head`;
      if (hasWasher) {
        hwKids.push({
          id: `${instPrefix}_washer`,
          op: 'add',
          blend: 0,
          suppressed: false,
          material: spec.fastenerMaterial,
          color: '#E2E8F0',
          transform: { translate: [lx, ly, lz + sizeSpec.washer_t * 0.5 - 0.05], rotate: childRot },
          kind: {
            type: 'cylinder',
            radius: sizeSpec.washer_od * 0.5,
            height: sizeSpec.washer_t,
            round: 0.1,
          },
        });
      }
      hwKids.push({
        id: headId,
        op: 'add',
        blend: 0,
        suppressed: false,
        material: spec.fastenerMaterial,
        color: '#94A3B8',
        transform: {
          translate: [lx, ly, lz + washerShiftZ + sizeSpec.head_h * 0.5 - 0.1],
          rotate: childRot,
        },
        kind: {
          type: 'cylinder',
          radius: sizeSpec.cap_head_d * 0.5,
          height: sizeSpec.head_h,
          round: 0.35,
        },
      });
      addClampLoad(seatId, tapBoreId, headId);
    } else if (spec.kind === 'hex_head_screw') {
      const seatId = `${instPrefix}_seat_spotface`;
      boreKids.push({
        id: seatId,
        op: boreChildOp,
        blend: 0,
        suppressed: false,
        transform: { translate: [lx, ly, lz - 0.2], rotate: childRot },
        kind: { type: 'cylinder', radius: sizeSpec.washer_od * 0.5, height: 0.6, round: 0 },
      });
      const tapBoreId = `${instPrefix}_tapped_bore`;
      boreKids.push({
        id: tapBoreId,
        op: boreChildOp,
        blend: 0,
        suppressed: false,
        transform: { translate: [lx, ly, lz - engH * 0.5], rotate: childRot },
        kind: { type: 'cylinder', radius: nutKind !== 'none' ? clearR : pilotR, height: engH + 1.0, round: 0 },
      });
      if (hasWasher) {
        hwKids.push({
          id: `${instPrefix}_washer`,
          op: 'add',
          blend: 0,
          suppressed: false,
          material: spec.fastenerMaterial,
          color: '#E2E8F0',
          transform: { translate: [lx, ly, lz + sizeSpec.washer_t * 0.5 - 0.05], rotate: childRot },
          kind: {
            type: 'cylinder',
            radius: sizeSpec.washer_od * 0.5,
            height: sizeSpec.washer_t,
            round: 0.1,
          },
        });
      }
      const hexHeadId = `${instPrefix}_hex_head`;
      hwKids.push({
        id: hexHeadId,
        op: 'add',
        blend: 0,
        suppressed: false,
        material: spec.fastenerMaterial,
        color: '#CBD5E1',
        transform: {
          translate: [lx, ly, lz + washerShiftZ + sizeSpec.head_h * 0.5 - 0.1],
          rotate: childRot,
        },
        kind: {
          type: 'extrude',
          profile: hexProfile(sizeSpec.hex_flats_s),
          height: sizeSpec.head_h,
        },
      });
      addClampLoad(seatId, tapBoreId, hexHeadId);
    } else if (spec.kind === 'hex_cap_bolt') {
      const cboreId = `${instPrefix}_counterbore`;
      const cboreDepth = hasWasher ? 0.4 : Math.max(1.2, sizeSpec.head_h * 0.45);
      boreKids.push({
        id: cboreId,
        op: boreChildOp,
        blend: 0,
        suppressed: false,
        transform: { translate: [lx, ly, lz - cboreDepth * 0.5 + 0.1], rotate: childRot },
        kind: {
          type: 'cylinder',
          radius: (hasWasher ? sizeSpec.washer_od : sizeSpec.cap_head_d) * 0.5 + 0.3,
          height: cboreDepth,
          round: 0,
        },
      });
      const tapBoreId = `${instPrefix}_tap_bore`;
      boreKids.push({
        id: tapBoreId,
        op: boreChildOp,
        blend: 0,
        suppressed: false,
        transform: { translate: [lx, ly, lz - engH * 0.5], rotate: childRot },
        kind: { type: 'cylinder', radius: nutKind !== 'none' ? clearR : pilotR, height: engH + 1.0, round: 0 },
      });
      if (hasWasher) {
        hwKids.push({
          id: `${instPrefix}_washer`,
          op: 'add',
          blend: 0,
          suppressed: false,
          material: spec.fastenerMaterial,
          color: '#E2E8F0',
          transform: {
            translate: [lx, ly, lz - (cboreDepth - 0.1) + sizeSpec.washer_t * 0.5],
            rotate: childRot,
          },
          kind: {
            type: 'cylinder',
            radius: sizeSpec.washer_od * 0.5,
            height: sizeSpec.washer_t,
            round: 0.1,
          },
        });
      }
      const capHeadId = `${instPrefix}_cap_head`;
      const headZ = lz - (cboreDepth - 0.1) + washerShiftZ + sizeSpec.head_h * 0.5;
      hwKids.push({
        id: capHeadId,
        op: 'add',
        blend: 0,
        suppressed: false,
        material: spec.fastenerMaterial,
        color: '#64748B',
        transform: { translate: [lx, ly, headZ], rotate: childRot },
        kind: {
          type: 'cylinder',
          radius: sizeSpec.cap_head_d * 0.5,
          height: sizeSpec.head_h,
          round: 0.25,
        },
      });
      hwKids.push({
        id: `${instPrefix}_allen_socket`,
        op: 'subtract',
        blend: 0,
        suppressed: false,
        transform: { translate: [lx, ly, headZ + sizeSpec.head_h * 0.25], rotate: childRot },
        kind: {
          type: 'extrude',
          profile: hexProfile(sizeSpec.allen_s),
          height: sizeSpec.head_h * 0.6,
        },
      });
      addClampLoad(cboreId, tapBoreId, capHeadId);
    }

    // If an associated nut (`hex_nut` or `nyloc_nut`) is attached to this bolt/screw:
    if (isBoltOrScrew && nutKind !== 'none') {
      const pocketDepth = Math.max(1.4, sizeSpec.nut_h * 0.65);
      const nutZ = lz - engH + sizeSpec.nut_h * 0.5;
      boreKids.push({
        id: `${instPrefix}_nut_pocket`,
        op: boreChildOp,
        blend: 0,
        suppressed: false,
        transform: { translate: [lx, ly, lz - engH + pocketDepth * 0.5 - 0.1], rotate: childRot },
        kind: {
          type: 'extrude',
          profile: hexProfile(sizeSpec.hex_flats_s + 0.35),
          height: pocketDepth + 0.4,
        },
      });
      hwKids.push({
        id: `${instPrefix}_shank`,
        op: 'add',
        blend: 0,
        suppressed: false,
        material: spec.fastenerMaterial,
        color: '#94A3B8',
        transform: { translate: [lx, ly, lz - engH * 0.5], rotate: childRot },
        kind: {
          type: 'cylinder',
          radius: majorR * 0.92,
          height: engH + sizeSpec.nut_h * 0.5,
          round: 0,
        },
      });
      hwKids.push({
        id: `${instPrefix}_nut_body`,
        op: 'add',
        blend: 0,
        suppressed: false,
        material: spec.fastenerMaterial,
        color: '#CBD5E1',
        transform: { translate: [lx, ly, nutZ], rotate: childRot },
        kind: {
          type: 'extrude',
          profile: hexProfile(sizeSpec.hex_flats_s),
          height: sizeSpec.nut_h,
        },
      });
      if (nutKind === 'nyloc_nut') {
        const nylonH = Math.max(1.1, sizeSpec.nut_h * 0.38);
        hwKids.push({
          id: `${instPrefix}_nylon_collar`,
          op: 'add',
          blend: 0,
          suppressed: false,
          material: 'pa12',
          color: '#38BDF8',
          transform: {
            translate: [lx, ly, nutZ - sizeSpec.nut_h * 0.5 - nylonH * 0.4],
            rotate: childRot,
          },
          kind: {
            type: 'cylinder',
            radius: sizeSpec.hex_flats_s * 0.46,
            height: nylonH,
            round: 0.3,
          },
        });
      }
    }

    if (spec.kind === 'washer') {
      const spotfaceId = `${instPrefix}_spotface`;
      boreKids.push({
        id: spotfaceId,
        op: boreChildOp,
        blend: 0,
        suppressed: false,
        transform: { translate: [lx, ly, lz - 0.15], rotate: childRot },
        kind: {
          type: 'cylinder',
          radius: sizeSpec.washer_od * 0.5 + 0.2,
          height: 0.5,
          round: 0,
        },
      });
      const clearBoreId = `${instPrefix}_clearance_bore`;
      boreKids.push({
        id: clearBoreId,
        op: boreChildOp,
        blend: 0,
        suppressed: false,
        transform: { translate: [lx, ly, lz - engH * 0.5], rotate: childRot },
        kind: { type: 'cylinder', radius: clearR, height: engH + 1.0, round: 0 },
      });
      const discId = `${instPrefix}_disc`;
      const wz = lz + sizeSpec.washer_t * 0.5 - 0.05;
      hwKids.push({
        id: discId,
        op: 'add',
        blend: 0,
        suppressed: false,
        material: spec.fastenerMaterial,
        color: '#94A3B8',
        transform: { translate: [lx, ly, wz], rotate: childRot },
        kind: {
          type: 'cylinder',
          radius: sizeSpec.washer_od * 0.5,
          height: sizeSpec.washer_t,
          round: 0.1,
        },
      });
      hwKids.push({
        id: `${instPrefix}_id_bore`,
        op: 'subtract',
        blend: 0,
        suppressed: false,
        transform: { translate: [lx, ly, wz], rotate: childRot },
        kind: { type: 'cylinder', radius: clearR, height: sizeSpec.washer_t + 0.4, round: 0 },
      });
      addClampLoad(spotfaceId, clearBoreId, discId);
    } else if (spec.kind === 'hex_nut' || spec.kind === 'nyloc_nut') {
      const nutPocketId = `${instPrefix}_nut_pocket`;
      const pocketDepth = Math.max(1.4, sizeSpec.nut_h * 0.65);
      boreKids.push({
        id: nutPocketId,
        op: boreChildOp,
        blend: 0,
        suppressed: false,
        transform: { translate: [lx, ly, lz - pocketDepth * 0.5 + 0.1], rotate: childRot },
        kind: {
          type: 'extrude',
          profile: hexProfile(sizeSpec.hex_flats_s + 0.3),
          height: pocketDepth,
        },
      });
      const boltBoreId = `${instPrefix}_bolt_bore`;
      boreKids.push({
        id: boltBoreId,
        op: boreChildOp,
        blend: 0,
        suppressed: false,
        transform: { translate: [lx, ly, lz - engH * 0.5], rotate: childRot },
        kind: { type: 'cylinder', radius: clearR, height: engH + 1.0, round: 0 },
      });
      const nutBodyId = `${instPrefix}_hex_body`;
      const nz = lz - (pocketDepth - 0.1) + sizeSpec.nut_h * 0.5;
      hwKids.push({
        id: nutBodyId,
        op: 'add',
        blend: 0,
        suppressed: false,
        material: spec.fastenerMaterial,
        color: '#CBD5E1',
        transform: { translate: [lx, ly, nz], rotate: childRot },
        kind: {
          type: 'extrude',
          profile: hexProfile(sizeSpec.hex_flats_s),
          height: sizeSpec.nut_h,
        },
      });
      let extraH = 0;
      if (spec.kind === 'nyloc_nut') {
        const nylonH = Math.max(1.2, sizeSpec.nut_h * 0.42);
        extraH = nylonH;
        hwKids.push({
          id: `${instPrefix}_nylon_insert`,
          op: 'add',
          blend: 0,
          suppressed: false,
          material: 'pa12',
          color: '#38BDF8',
          transform: {
            translate: [lx, ly, nz + sizeSpec.nut_h * 0.5 + nylonH * 0.45],
            rotate: childRot,
          },
          kind: {
            type: 'cylinder',
            radius: sizeSpec.hex_flats_s * 0.46,
            height: nylonH,
            round: 0.35,
          },
        });
      }
      hwKids.push({
        id: `${instPrefix}_internal_thread`,
        op: 'subtract',
        blend: 0,
        suppressed: false,
        transform: { translate: [lx, ly, nz + extraH * 0.5], rotate: childRot },
        kind: {
          type: 'cylinder',
          radius: pilotR,
          height: sizeSpec.nut_h + extraH + 1.0,
          round: 0,
        },
      });
      addClampLoad(nutPocketId, boltBoreId, nutBodyId);
    } else if (spec.kind === 'hook_clip_box') {
      // hook_clip_box
      const beamW = Math.max(5.0, sizeSpec.major_d * 1.6);
      const beamT = Math.max(1.6, sizeSpec.major_d * 0.45);
      const beamL = Math.max(10.0, spec.engagementMm);
      const undercutD = Math.max(0.9, beamT * 0.55);

      const catchSlotId = `${instPrefix}_catch_slot`;
      boreKids.push({
        id: catchSlotId,
        op: boreChildOp,
        blend: 0,
        suppressed: false,
        transform: { translate: [lx, ly, lz - 2.2], rotate: childRot },
        kind: {
          type: 'box',
          size: [beamT + undercutD + 0.5, beamW + 0.8, 3.2],
          round: 0.2,
        },
      });
      const armId = `${instPrefix}_cantilever_arm`;
      hwKids.push({
        id: armId,
        op: 'add',
        blend: 0.6,
        suppressed: false,
        color: '#38BDF8',
        transform: {
          translate: [lx + beamT * 0.5 + 0.25, ly, lz + beamL * 0.5 - 1.0],
          rotate: childRot,
        },
        kind: { type: 'box', size: [beamT, beamW, beamL], round: 0.35 },
      });
      const hookTipId = `${instPrefix}_hook_tip`;
      hwKids.push({
        id: hookTipId,
        op: 'add',
        blend: 0.25,
        suppressed: false,
        color: '#0284C7',
        transform: {
          translate: [lx + beamT * 0.5 + 0.25 - undercutD * 0.5, ly, lz + beamL - 2.0],
          rotate: childRot,
        },
        kind: { type: 'box', size: [beamT + undercutD, beamW, 2.6], round: 0.3 },
      });
      const regId = `${instPrefix}_snap_ledge`;
      draft.regions.push({
        id: regId,
        selectors: [
          { shape: hookTipId, face: 'nx' },
          { shape: catchSlotId, face: 'nz' },
        ],
      });
      draft.loads.push({
        id: `${instPrefix}_snap_load`,
        region: regId,
        case: 'default',
        kind: {
          type: 'force',
          vector: [-Math.min(45, Math.round(envelope.recommendedPreloadN)), 0, 0],
        },
      });
    }

    if (useGroup) {
      const boreGroupId = `${instPrefix}_bore`;
      const hwGroupId = instPrefix;
      const boreGroup: Node = {
        id: boreGroupId,
        op: 'subtract',
        blend: 0,
        suppressed: false,
        transform: { translate: [...surfacePt], rotate: [...rot] },
        kind: { type: 'group', children: boreKids, modifiers: [] },
      };
      const hwGroup: Node = {
        id: hwGroupId,
        op: 'add',
        blend: 0,
        suppressed: false,
        color: '#94A3B8',
        transform: { translate: [...surfacePt], rotate: [...rot] },
        kind: { type: 'group', children: hwKids, modifiers: [] },
      };
      createdNodes.push(boreGroup, hwGroup);
      createdGroupIds.push(boreGroupId, hwGroupId);
      if (!firstNewId) firstNewId = hwGroupId;
      if (!firstBoreGroupId) firstBoreGroupId = boreGroupId;
    } else {
      createdNodes.push(...boreKids, ...hwKids);
      if (!firstNewId && hwKids.length > 0) firstNewId = hwKids[0].id;
    }
  };

  if (pointsUV.length === 1) {
    buildOneInstance(basePrefix, pointsUV[0]);
  } else {
    pointsUV.forEach((ptUV, i) => {
      buildOneInstance(`${basePrefix}_${i + 1}`, ptUV);
    });
  }

  associateNodesWithHost(
    draft,
    targetNode?.id,
    createdNodes,
    spec.associateWithHostGroup === true,
  );

  // Wire parametric `let` surface dimensions (`_dx`/`_dy` or `_u`/`_v`) so editing them moves both bore and hardware groups
  if (bindLets && useGroup && firstNewId && firstBoreGroupId) {
    const pt0 = pointsUV[0];
    const fmtNum = (val: number) => Number(val.toFixed(4));
    if (mode === 'center') {
      const uName = `${basePrefix}_u`;
      const vName = `${basePrefix}_v`;
      upsertLetParam(draft, uName, fmtNum(pt0.u));
      upsertLetParam(draft, vName, fmtNum(pt0.v));
      const baseU = fmtNum(frame.faceCenter[frame.uAxisIndex]);
      const baseV = fmtNum(frame.faceCenter[frame.vAxisIndex]);
      const exprU = baseU === 0 ? uName : `${baseU} + ${uName}`;
      const exprV = baseV === 0 ? vName : `${baseV} + ${vName}`;
      for (const gid of [firstBoreGroupId, firstNewId]) {
        upsertConsumerExpr(draft, gid, 'at', frame.uAxisIndex, exprU);
        upsertConsumerExpr(draft, gid, 'at', frame.vAxisIndex, exprV);
      }
    } else {
      const dxName = `${basePrefix}_dx`;
      const dyName = `${basePrefix}_dy`;
      upsertLetParam(draft, dxName, fmtNum(pt0.dx));
      upsertLetParam(draft, dyName, fmtNum(pt0.dy));
      const baseU = fmtNum(frame.faceCenter[frame.uAxisIndex] - 0.5 * frame.faceWidth);
      const baseV = fmtNum(frame.faceCenter[frame.vAxisIndex] - 0.5 * frame.faceHeight);
      const exprU = baseU === 0 ? dxName : `${baseU} + ${dxName}`;
      const exprV = baseV === 0 ? dyName : `${baseV} + ${dyName}`;
      for (const gid of [firstBoreGroupId, firstNewId]) {
        upsertConsumerExpr(draft, gid, 'at', frame.uAxisIndex, exprU);
        upsertConsumerExpr(draft, gid, 'at', frame.vAxisIndex, exprV);
      }
    }
  }

  return {
    firstNewId,
    boreGroupId: firstBoreGroupId,
    createdGroupIds,
    envelope,
  };
}

export interface OpenStandardBenchmark {
  id: string;
  standardCode: string;
  isoCrossRef: string;
  openUrl: string;
  title: string;
  quickStandardRule: string;
  aprioriDerivation: string;
  metricLabel: string;
  standardValue: number;
  aprioriValue: number;
  discrepancyPct: number;
  regimeInsight: string;
}

export function evaluateOpenStandardBenchmarks(
  inp: FixingEnvelopeInput,
  activeSamples?: FailureSampleRow[],
  targetPf = 1e-6,
): OpenStandardBenchmark[] {
  const d = Math.max(1.5, inp.nominalDMm);
  const p = Math.max(0.25, inp.pitchMm);
  const lEng = Math.max(1.5 * p, inp.engagementLengthMm);
  const muT = Math.min(0.6, Math.max(0.04, inp.muThread ?? 0.14));
  const muH = Math.min(0.6, Math.max(0.04, inp.muHead ?? 0.15));

  // 1. FED-STD-H28 §2B / ASME B1.13M vs 60° equilateral fundamental triangle rederivation
  const asStandardMm2 = 0.25 * Math.PI * Math.pow(d - 0.938194 * p, 2);
  const hTriangle = (Math.sqrt(3) / 2) * p;
  const d2Apriori = d - 0.75 * hTriangle;
  const d3Apriori = d - (17 / 12) * hTriangle;
  const dsApriori = 0.5 * (d2Apriori + d3Apriori);
  const asAprioriMm2 = 0.25 * Math.PI * dsApriori * dsApriori;
  const errB1 = (Math.abs(asAprioriMm2 - asStandardMm2) / asStandardMm2) * 100;

  // 2. NASA-STD-5020B §4.2 / MIL-HDBK-60 Short-Form K*d*F vs Motosh Helical Inclined-Plane Integral
  const env = evaluateFixingTorqueEnvelope(inp);
  const preloadRefN = Math.max(100, env.recommendedPreloadN);
  const kStd = Math.abs(muT - 0.14) < 0.03 ? 0.193 : muT < 0.12 ? 0.165 : 0.215;
  const tStdNm = kStd * (d * 1e-3) * preloadRefN + env.threadFormingTorqueNm;
  const alphaN =
    inp.fixingKind === 'self_tapping_screw' ? (15 * Math.PI) / 180 : (30 * Math.PI) / 180;
  const r2 = 0.5 * d2Apriori;
  const rb = 0.65 * d;
  const tAprioriNm =
    preloadRefN * 1e-3 * (p / (2 * Math.PI) + (muT * r2) / Math.cos(alphaN) + muH * rb) +
    env.threadFormingTorqueNm;
  const errB2 = (Math.abs(tAprioriNm - tStdNm) / Math.max(1e-6, tStdNm)) * 100;

  // 3. NIST/SEMATECH e-Handbook §8.1.6.2 Weibull Parameter Recovery
  const samples =
    activeSamples && activeSamples.length >= 2 ? activeSamples : JWST_CRYO_OPTICS_COUPONS;
  const fit = fitFailureDistribution(samples, targetPf);
  const sigma0Catalog =
    inp.substrateMaterial === 'sic_jwst'
      ? 340.0
      : inp.substrateMaterial === 'alsi10mg'
        ? 303.0
        : inp.substrateMaterial === 'cf_woven_3k'
          ? 55.0
          : 60.0;
  const errB3 = (Math.abs(fit.weibullScaleSigma0Mpa - sigma0Catalog) / sigma0Catalog) * 100;

  // 4. NASA-HDBK-7005 & CARES/Life + Sopwith (1948) 1st-Thread Load Concentration Y_1
  const sopwithHandbook = 2.85;
  const lambda = Math.min(4.5, Math.max(0.6, lEng / (1.05 * d)));
  const sopwithApriori = Math.min(3.8, Math.max(1.35, lambda / Math.tanh(lambda)));
  const errB4 = (Math.abs(sopwithApriori - sopwithHandbook) / sopwithHandbook) * 100;

  return [
    {
      id: 'fed_std_h28_stress_area',
      standardCode: 'FED-STD-H28 §2B / ASME B1.13M',
      isoCrossRef: 'ISO 68-1 / ISO 898-1 §9.1.6',
      openUrl: 'https://assist.dla.mil/',
      title: 'Metric 60° Thread Tensile Stress Area A_s',
      quickStandardRule: 'Tabulated handbook formula: A_s = (π/4)·(d − 0.938194·P)²',
      aprioriDerivation:
        '60° equilateral triangle height H = (√3/2)·P averaging pitch diameter d₂ = d − (3√3/8)·P and root diameter d₃ = d − (17√3/24)·P → d_s = d − (13√3/24)·P = d − 0.938194·P',
      metricLabel: `A_s (mm²) at d=${d.toFixed(1)} mm, P=${p.toFixed(2)} mm`,
      standardValue: asStandardMm2,
      aprioriValue: asAprioriMm2,
      discrepancyPct: errB1,
      regimeInsight:
        'Exact geometric identity (0.000% discrepancy): the 0.938194 constant in ISO 898-1 and FED-STD-H28 is simply 13√3/24 from the 60° V-thread truncation geometry.',
    },
    {
      id: 'nasa_std_5020b_torque_preload',
      standardCode: 'NASA-STD-5020B §4.2 / MIL-HDBK-60',
      isoCrossRef: 'VDI 2230 Part 1 §5.4 / ISO 16047',
      openUrl: 'https://standards.nasa.gov/standard/NASA/NASA-STD-5020',
      title: 'Torque–Preload Nut Factor vs. Motosh Inclined-Plane Integral',
      quickStandardRule: `Short-form handbook rule: T = K·d·F_preload + T_prev (K=${kStd.toFixed(3)})`,
      aprioriDerivation:
        '3-surface virtual work integral: T = F·[P/(2π) + μ_t·r₂/cos(α_n) + μ_h·r_b] + T_form over lead angle β, flank angle α_n (15° Plastite vs 30° ISO), and bearing radius r_b',
      metricLabel: `T_rec (N·m) at F=${preloadRefN.toFixed(0)} N`,
      standardValue: tStdNm,
      aprioriValue: tAprioriNm,
      discrepancyPct: errB2,
      regimeInsight:
        'Standard K ≈ 0.20 agrees within ~2% on 60° metal threads, but diverges on 30° Plastite self-tapping screws (cos 15° vs cos 30° lowers flank wedging by 10.4%) or wide washers.',
    },
    {
      id: 'nist_sematech_8162_weibull',
      standardCode: 'NIST/SEMATECH e-Handbook §8.1.6.2',
      isoCrossRef: 'ISO 20501 / ASTM C1239 (Weibull Ceramics)',
      openUrl: 'https://www.itl.nist.gov/div898/handbook/apr/section1/apr162.htm',
      title: 'Weibull Fracture Parameter Recovery & Bernard Median Ranks',
      quickStandardRule: `Reference coupon baseline σ₀ = ${sigma0Catalog.toFixed(1)} MPa`,
      aprioriDerivation:
        'Weakest-link Poisson flaw integral P_f = 1 − exp(−(σ₁/σ₀)^m) linearized via ln(−ln(1−P_f)) = m·ln(σ₁) − m·ln(σ₀) with Bernard median rank F_i = (i−0.3)/(N+0.4)',
      metricLabel: `Recovered Scale σ₀ (MPa) (m=${fit.weibullModulusM.toFixed(2)}, R²=${(fit.weibullR2 * 100).toFixed(2)}%)`,
      standardValue: sigma0Catalog,
      aprioriValue: fit.weibullScaleSigma0Mpa,
      discrepancyPct: errB3,
      regimeInsight: `Least-squares regression on the active spreadsheet rows yields σ₀ = ${fit.weibullScaleSigma0Mpa.toFixed(1)} MPa and allowable stress ${fit.allowableStressAtTargetPfMpa.toFixed(1)} MPa at target P_f = ${targetPf.toExponential(0)}.`,
    },
    {
      id: 'nasa_hdbk_7005_sopwith_thread_load',
      standardCode: 'NASA-HDBK-7005 / CARES (TM-106316)',
      isoCrossRef: 'VDI 2230 §5.1 / Sopwith (1948)',
      openUrl: 'https://ntrs.nasa.gov/citations/20010021161',
      title: 'First-Engaged Thread Root Load Concentration Y₁',
      quickStandardRule: 'Empirical design rule: 1st thread carries ~2.85× average thread load',
      aprioriDerivation:
        'Coupled axial bolt-stretching vs nut-compression ODE d²F/dz² − (λ/L_eng)²·F = 0 yielding hyperbolic concentration Y₁ = λ·coth(λ) with λ = L_eng/(1.05·d)',
      metricLabel: `Y₁ = F_thread,1 / F_avg (L_eng=${lEng.toFixed(1)} mm)`,
      standardValue: sopwithHandbook,
      aprioriValue: sopwithApriori,
      discrepancyPct: errB4,
      regimeInsight:
        'Explains why 78% of fastener fatigue and brittle SiC boss fractures initiate at the very first engaged thread root despite adding engagement length beyond L_eng ≈ 1.5·d.',
    },
  ];
}

// ============================================================================
// Live Inspection & In-Place Reconfiguration of Existing Bolt/Screw Stacks
// ============================================================================

export interface FixingStackInspection {
  /** The hardware group ID (`<prefix>`, e.g. `hex_cap_bolt` or `hex_cap_bolt_1`) */
  hwGroupId: string;
  /** The companion bore group ID (`<prefix>_bore`) if present */
  boreGroupId?: string;
  /** The base prefix (`hex_cap_bolt`, `self_tapping_screw_2`, etc.) */
  basePrefix: string;
  /** Fixing kind (`hex_cap_bolt`, `hex_head_screw`, `self_tapping_screw`) */
  fixingKind: FixingKindId;
  /** Resolved metric size specification (`M2`..`M12`) */
  sizeSpec: FixingSizeSpec;
  /** Thread engagement / bolt grip length in mm */
  engagementMm: number;
  /** Fastener material ID (`steel_1020`, `ti6al4v`, etc.) */
  fastenerMaterial: string;
  /** Host substrate material ID (`petg`, `alsi10mg`, `sic_jwst`, etc.) */
  substrateMaterial: string;
  /** Whether an underhead bearing washer (`<prefix>_washer`) is currently present */
  includeWasher: boolean;
  /** Currently associated mated nut (`'none' | 'hex_nut' | 'nyloc_nut'`) */
  associatedNut: AssociatedNutKind;
  /** Full live torque & holdable-forces report for the current stack */
  envelope: FixingEnvelopeReport;
}

function findParentGroupId(nodes: Node[], childId: string): string | undefined {
  for (const n of nodes) {
    if (n.kind.type === 'group') {
      if (n.kind.children.some((c) => c.id === childId)) return n.id;
      const sub = findParentGroupId(n.kind.children, childId);
      if (sub) return sub;
    }
  }
  return undefined;
}

function detectBoltFixingKindFromPrefix(prefix: string, hwGroup: Node): FixingKindId | null {
  if (prefix.startsWith('hex_cap_bolt')) return 'hex_cap_bolt';
  if (prefix.startsWith('hex_head_screw')) return 'hex_head_screw';
  if (prefix.startsWith('self_tapping_screw')) return 'self_tapping_screw';
  if (hwGroup.kind.type === 'group') {
    const ids = hwGroup.kind.children.map((c) => c.id);
    if (ids.some((id) => id.endsWith('_cap_head') || id.endsWith('_allen_socket'))) {
      return 'hex_cap_bolt';
    }
    if (ids.some((id) => id.endsWith('_hex_head'))) {
      return 'hex_head_screw';
    }
    if (ids.some((id) => id.endsWith('_thread_helix'))) {
      return 'self_tapping_screw';
    }
  }
  return null;
}

export function inspectFixingGroupStack(
  doc: Document,
  selectedNodeId: string,
): FixingStackInspection | null {
  let candidateId = selectedNodeId;
  if (candidateId.endsWith('_bore')) {
    candidateId = candidateId.slice(0, -5);
  }
  let node = findNode(doc, candidateId);
  if (!node || node.kind.type !== 'group') {
    const parentId = findParentGroupId(doc.root, selectedNodeId);
    if (parentId) {
      const cleanParent = parentId.endsWith('_bore') ? parentId.slice(0, -5) : parentId;
      node = findNode(doc, cleanParent);
      candidateId = cleanParent;
    }
  }
  if (!node || node.kind.type !== 'group') return null;

  const fixingKind = detectBoltFixingKindFromPrefix(candidateId, node);
  if (!fixingKind) return null;

  const basePrefix = candidateId.replace(/_\d+$/, '');
  const boreNode = findNode(doc, `${candidateId}_bore`);
  const kids: Node[] = node.kind.children;
  const boreKids: Node[] = boreNode && boreNode.kind.type === 'group' ? boreNode.kind.children : [];

  // Resolve sizeSpec from `<basePrefix>_d` or `<candidateId>_d` in `doc.lets`, or fallback to head radius
  const dLet = (doc.lets ?? []).find(
    (l) => l.name === `${candidateId}_d` || l.name === `${basePrefix}_d`,
  )?.value;
  let sizeSpec = FIXING_SIZES[3]; // default M4
  if (typeof dLet === 'number') {
    const match = FIXING_SIZES.find((s) => Math.abs(s.major_d - dLet) < 0.15);
    if (match) sizeSpec = match;
  } else {
    const headChild = kids.find(
      (c: Node) => c.id.endsWith('_cap_head') || c.id.endsWith('_head'),
    );
    if (headChild && headChild.kind.type === 'cylinder') {
      const capD = headChild.kind.radius * 2;
      const match = FIXING_SIZES.find((s) => Math.abs(s.cap_head_d - capD) < 0.35);
      if (match) sizeSpec = match;
    }
  }

  // Resolve engagement length from bore cylinder height
  let engagementMm = Math.round(sizeSpec.major_d * 2.2 * 10) / 10;
  const tapBore = boreKids.find(
    (c: Node) => c.id.endsWith('_tap_bore') || c.id.endsWith('_tapped_bore'),
  );
  if (tapBore && tapBore.kind.type === 'cylinder') {
    engagementMm = Math.max(3, Math.round((tapBore.kind.height - 1.0) * 10) / 10);
  }

  const includeWasher = kids.some((c: Node) => c.id === `${candidateId}_washer` && !c.suppressed);
  const hasNyloc = kids.some((c: Node) => c.id === `${candidateId}_nylon_collar` && !c.suppressed);
  const hasNutBody = kids.some((c: Node) => c.id === `${candidateId}_nut_body` && !c.suppressed);
  const associatedNut: AssociatedNutKind = hasNyloc
    ? 'nyloc_nut'
    : hasNutBody
      ? 'hex_nut'
      : 'none';

  const headMat =
    kids.find(
      (c: Node) =>
        c.id.endsWith('_cap_head') ||
        c.id.endsWith('_hex_head') ||
        c.id.endsWith('_head'),
    )?.material ?? 'steel_1020';
  const substrateMaterial = doc.material || 'petg';
  const regime = classifySubstrateRegime(substrateMaterial);

  const envelope = evaluateFixingTorqueEnvelope({
    fixingKind,
    nominalDMm: sizeSpec.major_d,
    pitchMm: sizeSpec.pitch,
    engagementLengthMm: engagementMm,
    bossOdMm: sizeSpec.boss_od,
    substrateMaterial,
    fastenerMaterial: headMat,
    vibrationG: 3.5,
    shockG: 8.75,
    payloadMassKg: 0.45,
    numFixings: 1,
    deltaTempC: 0,
    serviceHours: 1000,
    targetFailureProb: 1e-3,
    muThread: regime === 'plastic_boss' ? 0.18 : 0.14,
    muHead: regime === 'plastic_boss' ? 0.2 : 0.15,
    muJoint: 0.25,
    includeWasher,
    associatedNut,
  });

  return {
    hwGroupId: candidateId,
    boreGroupId: boreNode?.id,
    basePrefix,
    fixingKind,
    sizeSpec,
    engagementMm,
    fastenerMaterial: headMat,
    substrateMaterial,
    includeWasher,
    associatedNut,
    envelope,
  };
}

export function reconfigureExistingBoltStack(
  draft: Document,
  selectedNodeId: string,
  opts: { includeWasher: boolean; associatedNut: AssociatedNutKind; fastenerMaterial?: string },
): FixingStackInspection | null {
  const info = inspectFixingGroupStack(draft, selectedNodeId);
  if (!info) return null;

  const hwGroup = findNode(draft, info.hwGroupId);
  if (!hwGroup || hwGroup.kind.type !== 'group') return null;
  const boreGroup = info.boreGroupId ? findNode(draft, info.boreGroupId) : undefined;

  const { sizeSpec, engagementMm, substrateMaterial, fixingKind } = info;
  const fastenerMaterial = opts.fastenerMaterial ?? info.fastenerMaterial;
  ensureFixingMaterialInDoc(draft, fastenerMaterial);
  for (const child of hwGroup.kind.children) {
    if (child.op === 'add' && !child.id.endsWith('_nylon_collar')) {
      child.material = fastenerMaterial;
    }
  }
  const instPrefix = info.hwGroupId;
  const hasWasher = opts.includeWasher;
  const nutKind = opts.associatedNut;
  const washerShiftZ = hasWasher ? sizeSpec.washer_t : 0;
  const engH = Math.max(3, engagementMm);
  const regime = classifySubstrateRegime(substrateMaterial);
  const pilotD = regime === 'plastic_boss' ? sizeSpec.pilot_d_plastic : sizeSpec.pilot_d_metal;
  const pilotR = Math.round(pilotD * 0.5 * 1000) / 1000;
  const clearR = sizeSpec.clearance_d * 0.5;
  const majorR = sizeSpec.major_d * 0.5;

  // 1. Remove any existing dynamic washer or nut stack children from hwGroup and boreGroup
  hwGroup.kind.children = hwGroup.kind.children.filter(
    (c: Node) =>
      c.id !== `${instPrefix}_washer` &&
      c.id !== `${instPrefix}_shank` &&
      c.id !== `${instPrefix}_nut_body` &&
      c.id !== `${instPrefix}_nylon_collar`,
  );
  if (boreGroup && boreGroup.kind.type === 'group') {
    boreGroup.kind.children = boreGroup.kind.children.filter(
      (c: Node) => c.id !== `${instPrefix}_nut_pocket`,
    );
  }

  // 2. Update head Z elevation and counterbore/tap bore dimensions
  if (fixingKind === 'hex_cap_bolt') {
    const cboreDepth = hasWasher ? 0.4 : Math.max(1.2, sizeSpec.head_h * 0.45);
    if (boreGroup && boreGroup.kind.type === 'group') {
      const cbore = boreGroup.kind.children.find((c: Node) => c.id === `${instPrefix}_counterbore`);
      if (cbore && cbore.kind.type === 'cylinder') {
        cbore.kind.radius = (hasWasher ? sizeSpec.washer_od : sizeSpec.cap_head_d) * 0.5 + 0.3;
        cbore.kind.height = cboreDepth;
        cbore.transform.translate[2] = -cboreDepth * 0.5 + 0.1;
      }
      const tapBore = boreGroup.kind.children.find((c: Node) => c.id === `${instPrefix}_tap_bore`);
      if (tapBore && tapBore.kind.type === 'cylinder') {
        tapBore.kind.radius = nutKind !== 'none' ? clearR : pilotR;
      }
    }
    const headZ = -(cboreDepth - 0.1) + washerShiftZ + sizeSpec.head_h * 0.5;
    const capHead = hwGroup.kind.children.find((c: Node) => c.id === `${instPrefix}_cap_head`);
    if (capHead) capHead.transform.translate[2] = headZ;
    const allen = hwGroup.kind.children.find((c: Node) => c.id === `${instPrefix}_allen_socket`);
    if (allen) allen.transform.translate[2] = headZ + sizeSpec.head_h * 0.25;
    if (hasWasher) {
      hwGroup.kind.children.unshift({
        id: `${instPrefix}_washer`,
        op: 'add',
        blend: 0,
        suppressed: false,
        material: fastenerMaterial,
        color: '#E2E8F0',
        transform: {
          translate: [0, 0, -(cboreDepth - 0.1) + sizeSpec.washer_t * 0.5],
          rotate: [0, 0, 0],
        },
        kind: {
          type: 'cylinder',
          radius: sizeSpec.washer_od * 0.5,
          height: sizeSpec.washer_t,
          round: 0.1,
        },
      });
    }
  } else {
    // self_tapping_screw or hex_head_screw
    if (boreGroup && boreGroup.kind.type === 'group') {
      const tapBore = boreGroup.kind.children.find(
        (c: Node) => c.id === `${instPrefix}_tap_bore` || c.id === `${instPrefix}_tapped_bore`,
      );
      if (tapBore && tapBore.kind.type === 'cylinder') {
        tapBore.kind.radius = nutKind !== 'none' ? clearR : pilotR;
      }
    }
    const headNode = hwGroup.kind.children.find(
      (c: Node) => c.id === `${instPrefix}_head` || c.id === `${instPrefix}_hex_head`,
    );
    if (headNode) {
      headNode.transform.translate[2] = washerShiftZ + sizeSpec.head_h * 0.5 - 0.1;
    }
    if (hasWasher) {
      hwGroup.kind.children.unshift({
        id: `${instPrefix}_washer`,
        op: 'add',
        blend: 0,
        suppressed: false,
        material: fastenerMaterial,
        color: '#E2E8F0',
        transform: {
          translate: [0, 0, sizeSpec.washer_t * 0.5 - 0.05],
          rotate: [0, 0, 0],
        },
        kind: {
          type: 'cylinder',
          radius: sizeSpec.washer_od * 0.5,
          height: sizeSpec.washer_t,
          round: 0.1,
        },
      });
    }
  }

  // 3. Add associated nut (`hex_nut` or `nyloc_nut`) if enabled
  if (nutKind !== 'none') {
    const pocketDepth = Math.max(1.4, sizeSpec.nut_h * 0.65);
    const nutZ = -engH + sizeSpec.nut_h * 0.5;
    if (boreGroup && boreGroup.kind.type === 'group') {
      boreGroup.kind.children.push({
        id: `${instPrefix}_nut_pocket`,
        op: 'add',
        blend: 0,
        suppressed: false,
        transform: { translate: [0, 0, -engH + pocketDepth * 0.5 - 0.1], rotate: [0, 0, 0] },
        kind: {
          type: 'extrude',
          profile: hexProfile(sizeSpec.hex_flats_s + 0.35),
          height: pocketDepth + 0.4,
        },
      });
    }
    hwGroup.kind.children.push({
      id: `${instPrefix}_shank`,
      op: 'add',
      blend: 0,
      suppressed: false,
      material: fastenerMaterial,
      color: '#94A3B8',
      transform: { translate: [0, 0, -engH * 0.5], rotate: [0, 0, 0] },
      kind: {
        type: 'cylinder',
        radius: majorR * 0.92,
        height: engH + sizeSpec.nut_h * 0.5,
        round: 0,
      },
    });
    hwGroup.kind.children.push({
      id: `${instPrefix}_nut_body`,
      op: 'add',
      blend: 0,
      suppressed: false,
      material: fastenerMaterial,
      color: '#CBD5E1',
      transform: { translate: [0, 0, nutZ], rotate: [0, 0, 0] },
      kind: {
        type: 'extrude',
        profile: hexProfile(sizeSpec.hex_flats_s),
        height: sizeSpec.nut_h,
      },
    });
    if (nutKind === 'nyloc_nut') {
      const nylonH = Math.max(1.1, sizeSpec.nut_h * 0.38);
      hwGroup.kind.children.push({
        id: `${instPrefix}_nylon_collar`,
        op: 'add',
        blend: 0,
        suppressed: false,
        material: 'pa12',
        color: '#38BDF8',
        transform: {
          translate: [0, 0, nutZ - sizeSpec.nut_h * 0.5 - nylonH * 0.4],
          rotate: [0, 0, 0],
        },
        kind: {
          type: 'cylinder',
          radius: sizeSpec.hex_flats_s * 0.46,
          height: nylonH,
          round: 0.3,
        },
      });
    }
  }

  // 4. Recompute envelope and update `let` parameters and preload load
  const updated = inspectFixingGroupStack(draft, info.hwGroupId);
  if (updated) {
    const env = updated.envelope;
    upsertLetParam(draft, `${info.basePrefix}_torque_nm`, Number(env.recommendedTorqueNm.toFixed(3)));
    upsertLetParam(draft, `${info.basePrefix}_preload_n`, Number(env.recommendedPreloadN.toFixed(1)));
    upsertLetParam(
      draft,
      `${info.basePrefix}_f_sep_n`,
      Math.round(env.holdableForces.maxAxialLoadBeforeGappingN),
    );
    upsertLetParam(
      draft,
      `${info.basePrefix}_f_slip_n`,
      Math.round(env.holdableForces.maxTransverseFrictionSlipN),
    );

    const loadEntry = draft.loads.find(
      (ld) => ld.id === `${instPrefix}_preload` || ld.id === `${info.basePrefix}_preload`,
    );
    if (loadEntry && loadEntry.kind.type === 'force') {
      const vec = loadEntry.kind.vector;
      const mag = Math.hypot(vec[0], vec[1], vec[2]);
      const newMag = Math.max(5, Math.round(env.recommendedPreloadN));
      if (mag > 1e-6) {
        loadEntry.kind.vector = [
          Math.round((vec[0] / mag) * newMag),
          Math.round((vec[1] / mag) * newMag),
          Math.round((vec[2] / mag) * newMag),
        ];
      }
    }
  }

  return updated;
}

