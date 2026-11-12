import type { Document, Face, ProfileVertex } from './types';
import { ensureContinuumMaterialInDoc } from './continuum';
import {
  applyDocCommand,
  evaluateFixingBenchmarksWasm,
  evaluateFixingEnvelopeWasm,
  evaluateFixingSnapFitWasm,
  fitFixingFailureDistributionWasm,
  inspectFixingStackWasm,
} from './wasm';

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
  weibullSigma0Mpa?: number;
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

const CATALOG_WEIBULL_SIGMA0_MPA: Record<string, number> = {
  petg: 60.0,
  pla: 65.0,
  abs: 51.0,
  pa12: 57.5,
  nylon_pa66: 96.0,
  pc_lexan: 79.5,
  pom_delrin: 87.0,
  peek_cf30: 128.0,
  alsi10mg: 303.0,
  al7075_t6: 662.0,
  steel_4_8: 422.0,
  steel_8_8: 842.0,
  steel_10_9: 1185.0,
  steel_12_9: 1420.0,
  ss304_a2_70: 592.0,
  ss316l: 618.0,
  ss17_4ph: 1540.0,
  ti64: 1265.0,
  in718: 1290.0,
  a286_superalloy: 862.0,
  brass_cuZn39pb3: 342.0,
  bronze_c65100: 500.0,
  cucrzr: 553.0,
  az91d_mg: 211.0,
  cf_woven_3k: 55.0,
  fr4_g10: 49.8,
  sic_jwst: 340.0,
  alumina_al2o3: 280.0,
  nbk7_glass: 70.0,
};

export function findSubstrateMaterial(
  matId: string,
): (FixingMaterialOption & { weibullSigma0Mpa: number }) | undefined {
  const opt =
    SUBSTRATE_MATERIAL_GROUPS.flatMap((g) => g.options).find((m) => m.id === matId) ??
    findFixingMaterialOption(matId);
  if (!opt) return undefined;
  const regime = classifySubstrateRegime(opt.id);
  const fallbackSigma0 =
    regime === 'brittle_weibull_optics'
      ? opt.yield_strength
      : regime === 'woven_composite'
        ? Number((opt.yield_z * 1.31).toFixed(1))
        : regime === 'metal_housing'
          ? Number((opt.yield_strength * 1.317).toFixed(1))
          : Number((opt.yield_strength * 1.277).toFixed(1));
  return {
    ...opt,
    weibullSigma0Mpa: opt.weibullSigma0Mpa ?? CATALOG_WEIBULL_SIGMA0_MPA[opt.id] ?? fallbackSigma0,
  };
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

// ============================================================================
// Authoritative Rust WASM Fastener & Preloaded-Joint Mechanics Wrappers
// (`crates/fieldsmith-doc/src/fixings.rs`)
// ============================================================================

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
  includeWasher?: boolean;
  associatedNut?: AssociatedNutKind;
  weibullMOverride?: number;
  weibullSigma0OverrideMpa?: number;
}

export interface FixingHoldableForces {
  axialPreloadAtRecN: number;
  residualClampAfterCreepN: number;
  maxAxialLoadBeforeGappingN: number;
  maxTransverseFrictionSlipN: number;
  maxShankShearPinN: number;
  maxThreadPulloutN: number;
  maxUnderheadBearingN: number;
  maxBoltTensileYieldN: number;
  safeWorkingAxialLoadN: number;
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

function mapEnvelopeReport(raw: Record<string, any>): FixingEnvelopeReport {
  const hf = raw.holdable_forces ?? {};
  const ml = raw.mode_limits_nm ?? {};
  return {
    substrateRegime: raw.substrate_regime as SubstrateRegimeId,
    substrateRegimeLabel: String(raw.substrate_regime_label ?? ''),
    governingFailureMode: String(raw.governing_failure_mode ?? ''),
    includeWasher: Boolean(raw.include_washer),
    associatedNut: (raw.associated_nut ?? 'none') as AssociatedNutKind,
    bearingAreaMm2: Number(raw.bearing_area_mm2 ?? 0),
    bearingPressureAtRecMpa: Number(raw.bearing_pressure_at_rec_mpa ?? 0),
    motoshTorqueArmMm: Number(raw.motosh_torque_arm_mm ?? 0),
    nutFactorK: Number(raw.nut_factor_k ?? 0),
    threadFormingTorqueNm: Number(raw.thread_forming_torque_nm ?? 0),
    minResidualClampN: Number(raw.min_residual_clamp_n ?? 0),
    minInstallPreloadN: Number(raw.min_install_preload_n ?? 0),
    creepRetentionFactor: Number(raw.creep_retention_factor ?? 1),
    thermalPreloadShiftN: Number(raw.thermal_preload_shift_n ?? 0),
    minTorqueNm: Number(raw.min_torque_nm ?? 0),
    maxAllowablePreloadN: Number(raw.max_allowable_preload_n ?? 0),
    maxTorqueNm: Number(raw.max_torque_nm ?? 0),
    recommendedTorqueNm: Number(raw.recommended_torque_nm ?? 0),
    recommendedPreloadN: Number(raw.recommended_preload_n ?? 0),
    torqueWindowRatio: Number(raw.torque_window_ratio ?? 0),
    peakPrincipalStressMpa: Number(raw.peak_principal_stress_mpa ?? 0),
    weibullFailureProbAtRec: Number(raw.weibull_failure_prob_at_rec ?? 0),
    weibullModulusM: Number(raw.weibull_modulus_m ?? 0),
    weibullScaleSigma0Mpa: Number(raw.weibull_scale_sigma0_mpa ?? 0),
    holdableForces: {
      axialPreloadAtRecN: Number(hf.axial_preload_at_rec_n ?? 0),
      residualClampAfterCreepN: Number(hf.residual_clamp_after_creep_n ?? 0),
      maxAxialLoadBeforeGappingN: Number(hf.max_axial_load_before_gapping_n ?? 0),
      maxTransverseFrictionSlipN: Number(hf.max_transverse_friction_slip_n ?? 0),
      maxShankShearPinN: Number(hf.max_shank_shear_pin_n ?? 0),
      maxThreadPulloutN: Number(hf.max_thread_pullout_n ?? 0),
      maxUnderheadBearingN: Number(hf.max_underhead_bearing_n ?? 0),
      maxBoltTensileYieldN: Number(hf.max_bolt_tensile_yield_n ?? 0),
      safeWorkingAxialLoadN: Number(hf.safe_working_axial_load_n ?? 0),
      safeWorkingShearLoadN: Number(hf.safe_working_shear_load_n ?? 0),
    },
    modeLimitsNm: {
      weibullPrincipalFractureNm: Number(ml.weibull_principal_fracture_nm ?? 0),
      bossCreepOrHoopSplitNm: Number(ml.boss_creep_or_hoop_split_nm ?? 0),
      substrateThreadStripNm: Number(ml.substrate_thread_strip_nm ?? 0),
      fastenerCoreYieldNm: Number(ml.fastener_core_yield_nm ?? 0),
      compositeInterlaminarPeelNm: Number(ml.composite_interlaminar_peel_nm ?? 0),
      underheadBearingCrushNm: Number(ml.underhead_bearing_crush_nm ?? 0),
    },
  };
}

export function evaluateFixingTorqueEnvelope(
  inp: FixingEnvelopeInput,
  doc?: Document,
): FixingEnvelopeReport {
  const raw = evaluateFixingEnvelopeWasm(inp as unknown as Record<string, unknown>, doc);
  return mapEnvelopeReport(raw);
}

export interface FailureSampleRow {
  id: string;
  torqueNm: number;
  principalStressMpa: number;
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
  const raw = fitFixingFailureDistributionWasm(
    samples as unknown as Record<string, unknown>[],
    targetPf,
  ) as Record<string, any>;
  const pts: Record<string, any>[] = Array.isArray(raw.points) ? raw.points : [];
  return {
    weibullModulusM: Number(raw.weibull_modulus_m ?? 0),
    weibullScaleSigma0Mpa: Number(raw.weibull_scale_sigma0_mpa ?? 0),
    weibullScaleT0Nm: Number(raw.weibull_scale_t0_nm ?? 0),
    weibullR2: Number(raw.weibull_r2 ?? 1),
    lognormalMuLn: Number(raw.lognormal_mu_ln ?? 0),
    lognormalSigmaLn: Number(raw.lognormal_sigma_ln ?? 0),
    allowableStressAtTargetPfMpa: Number(raw.allowable_stress_at_target_pf_mpa ?? 0),
    allowableTorqueAtTargetPfNm: Number(raw.allowable_torque_at_target_pf_nm ?? 0),
    points: pts.map((p) => ({
      id: String(p.id ?? ''),
      torqueNm: Number(p.torque_nm ?? 0),
      stressMpa: Number(p.stress_mpa ?? 0),
      pf: Number(p.pf ?? 0),
      lnStress: Number(p.ln_stress ?? 0),
      lnTorque: Number(p.ln_torque ?? 0),
      weibullY: Number(p.weibull_y ?? 0),
      modeNote: String(p.mode_note ?? ''),
    })),
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
  doc?: Document,
): SnapFitMechanicsReport {
  const raw = evaluateFixingSnapFitWasm(
    nominalDMm,
    engagementLengthMm,
    substrateMaterial,
    muJoint,
    doc,
  ) as Record<string, any>;
  return {
    beamLengthMm: Number(raw.beam_length_mm ?? 0),
    beamWidthMm: Number(raw.beam_width_mm ?? 0),
    beamThicknessMm: Number(raw.beam_thickness_mm ?? 0),
    undercutDepthMm: Number(raw.undercut_depth_mm ?? 0),
    insertionForcePushN: Number(raw.insertion_force_push_n ?? 0),
    retentionForcePullN: Number(raw.retention_force_pull_n ?? 0),
    rootFlexuralStrainPct: Number(raw.root_flexural_strain_pct ?? 0),
    allowableStrainPct: Number(raw.allowable_strain_pct ?? 0),
    strainSafetyFactor: Number(raw.strain_safety_factor ?? 0),
  };
}

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
  dxEdgeMm?: number;
  dyEdgeMm?: number;
  uOffsetMm?: number;
  vOffsetMm?: number;
  flangeInsetXMm?: number;
  flangeInsetYMm?: number;
  boltCircleRadiusMm?: number;
  boltCircleCount?: number;
  boltCircleStartDeg?: number;
  customPointsUV?: { u: number; v: number }[];
  groupInTree?: boolean;
  associateWithHostGroup?: boolean;
  bindSurfaceLets?: boolean;
  includeWasher?: boolean;
  associatedNut?: AssociatedNutKind;
  preloadCv?: number;
  fastenerMaterial: string;
  substrateMaterial?: string;
  vibrationG: number;
  payloadMassKg: number;
}

export function insertFixingIntoDocument(
  draft: Document,
  spec: FixingInsertSpec,
): {
  firstNewId: string;
  boreGroupId: string;
  createdGroupIds: string[];
  envelope: FixingEnvelopeReport;
} {
  const { doc: nextDoc, outcome } = applyDocCommand(draft, {
    cmd: 'insert_fixing',
    request: spec as unknown as Record<string, unknown>,
  });
  Object.assign(draft, nextDoc);
  const f = (outcome.fixing ?? {}) as Record<string, any>;
  return {
    firstNewId: String(f.first_new_id ?? ''),
    boreGroupId: String(f.bore_group_id ?? ''),
    createdGroupIds: Array.isArray(f.created_group_ids)
      ? f.created_group_ids.map(String)
      : [],
    envelope: mapEnvelopeReport(f.envelope ?? {}),
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
  const rows = evaluateFixingBenchmarksWasm(
    inp as unknown as Record<string, unknown>,
    activeSamples as unknown as Record<string, unknown>[] | undefined,
    targetPf,
  );
  return rows.map((r: Record<string, any>) => ({
    id: String(r.id ?? ''),
    standardCode: String(r.standard_code ?? ''),
    isoCrossRef: String(r.iso_cross_ref ?? ''),
    openUrl: String(r.open_url ?? ''),
    title: String(r.title ?? ''),
    quickStandardRule: String(r.quick_standard_rule ?? ''),
    aprioriDerivation: String(r.apriori_derivation ?? ''),
    metricLabel: String(r.metric_label ?? ''),
    standardValue: Number(r.standard_value ?? 0),
    aprioriValue: Number(r.apriori_value ?? 0),
    discrepancyPct: Number(r.discrepancy_pct ?? 0),
    regimeInsight: String(r.regime_insight ?? ''),
  }));
}

export interface FixingStackInspection {
  hwGroupId: string;
  boreGroupId?: string;
  basePrefix: string;
  fixingKind: FixingKindId;
  sizeSpec: FixingSizeSpec;
  engagementMm: number;
  fastenerMaterial: string;
  substrateMaterial: string;
  includeWasher: boolean;
  associatedNut: AssociatedNutKind;
  envelope: FixingEnvelopeReport;
}

function mapStackInspection(raw: Record<string, any>): FixingStackInspection {
  return {
    hwGroupId: String(raw.hw_group_id ?? ''),
    boreGroupId: raw.bore_group_id ? String(raw.bore_group_id) : undefined,
    basePrefix: String(raw.base_prefix ?? ''),
    fixingKind: raw.fixing_kind as FixingKindId,
    sizeSpec: raw.size_spec as FixingSizeSpec,
    engagementMm: Number(raw.engagement_mm ?? 8),
    fastenerMaterial: String(raw.fastener_material ?? 'ti64'),
    substrateMaterial: String(raw.substrate_material ?? 'petg'),
    includeWasher: Boolean(raw.include_washer),
    associatedNut: (raw.associated_nut ?? 'none') as AssociatedNutKind,
    envelope: mapEnvelopeReport(raw.envelope ?? {}),
  };
}

export function inspectFixingGroupStack(
  doc: Document,
  selectedNodeId: string,
): FixingStackInspection | null {
  const raw = inspectFixingStackWasm(doc, selectedNodeId);
  if (!raw) return null;
  return mapStackInspection(raw);
}

export function reconfigureExistingBoltStack(
  draft: Document,
  selectedNodeId: string,
  opts: { includeWasher: boolean; associatedNut: AssociatedNutKind; fastenerMaterial?: string },
): FixingStackInspection | null {
  const { doc: nextDoc, outcome } = applyDocCommand(draft, {
    cmd: 'reconfigure_fixing',
    request: {
      selected_node_id: selectedNodeId,
      include_washer: opts.includeWasher,
      associated_nut: opts.associatedNut,
      fastener_material: opts.fastenerMaterial,
    },
  });
  Object.assign(draft, nextDoc);
  if (!outcome.fixing_stack) return null;
  return mapStackInspection(outcome.fixing_stack);
}
