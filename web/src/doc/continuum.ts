// Continuum Phase & Constitutive Material Registry + Automatic Field Equation Activation Engine.
// Bridges Shape (1) volume materials and Setup (2) boundary conditions to active PDEs
// and generates live, stage-aware Applied Mathematics specifications for Theory (6).

import type { Document, Material, Node } from './types';
import { walk } from './types';
import { applyAddrs } from './param';
import { materialLibrary } from './wasm';

export type ContinuumPhaseId =
  | 'solid_elastic'
  | 'viscous_melt'
  | 'liquid_metal'
  | 'optical_medium'
  | 'phase_change_fuel'
  | 'reactive_gas';

export interface ContinuumMaterialSpec {
  id: string;
  name: string;
  phase: ContinuumPhaseId;
  phaseLabel: string;
  badgeColor: string;
  defaultHexColor: string;
  /** Governing constitutive law in terminal-friendly Unicode. */
  constitutiveEquation: string;
  /** Key physical parameters displayed in the Shape (1) Inspector and Theory (6) table. */
  parameters: { symbol: string; label: string; value: string; unit: string }[];
  /** Underlying Material entry stored in doc.custom_materials so WASM .fsm parser/solver works natively. */
  fsmMaterial: Material;
}

export const CONTINUUM_PHASE_META: Record<
  ContinuumPhaseId,
  { title: string; shortBadge: string; color: string; summary: string }
> = {
  solid_elastic: {
    title: 'Solid Structural & Thermo-Elastic (Hooke + Fourier)',
    shortBadge: 'Solid Elastic / Thermo-Elastic',
    color: '#3b82f6',
    summary: '3D P2 linear elasticity and steady Fourier heat conduction with Duhamel–Neumann thermal strain.',
  },
  viscous_melt: {
    title: 'Viscous & Viscoelastic Melts (VFT Extensional Stokes)',
    shortBadge: 'VFT Viscous Melt',
    color: '#f59e0b',
    summary: 'Non-Newtonian temperature-dependent Vogel–Fulcher–Tammann (VFT) extensional sheet rheology and capillary–gravity equilibrium.',
  },
  liquid_metal: {
    title: 'Liquid Metals & Turbulent MHD Fluids (Navier–Stokes VMS)',
    shortBadge: 'Liquid Metal · VMS + MHD',
    color: '#06b6d4',
    summary: 'Depth-averaged turbulent Navier–Stokes (Variational Multiscale) with thermal buoyancy and Lorentz electromagnetic induction thrust.',
  },
  optical_medium: {
    title: 'Optical Refractive Media (Snell–Descartes + Fresnel)',
    shortBadge: 'Optical Dielectric · Snell/Fresnel',
    color: '#38bdf8',
    summary: 'Refractive ray-tracing across curved interfaces, Fresnel reflection losses, caustic focal irradiance, and thermophoretic soot extinction.',
  },
  phase_change_fuel: {
    title: 'Phase-Change & Pyrolyzing Fuels (Stefan + Darcy Wick)',
    shortBadge: 'Phase-Change / Pyrolysis Fuel',
    color: '#f97316',
    summary: 'Enthalpy-porosity Stefan solid–liquid phase change, porous capillary Darcy wick transport, and endothermic vapor pyrolysis.',
  },
  reactive_gas: {
    title: 'Buoyant Plumes, Sprays & Reactive Gases (Low-Mach Navier–Stokes + Arrhenius)',
    shortBadge: 'Reactive Gas / Spray Continuum',
    color: '#ef4444',
    summary: 'Buoyancy-driven low-Mach flow, 1-step Arrhenius combustion kinetics, interfacial Navier-slip gas cushions, and Rosner soot thermophoresis.',
  },
};

export const MULTIPHYSICS_CONTINUUM_MATERIALS: ContinuumMaterialSpec[] = [
  {
    id: 'molten_tin',
    name: 'Molten Tin Bath Sn(l) (Turbulent MHD Liquid Metal)',
    phase: 'liquid_metal',
    phaseLabel: 'Liquid Metal · VMS + MHD',
    badgeColor: '#06b6d4',
    defaultHexColor: '#64748b',
    constitutiveEquation:
      'τ_Sn = μ_eff(T) (∇u + ∇uᵀ),   ρ_Sn(T) = ρ₀ [1 − β_Sn (T − T_ref)],   F_EM = J × B',
    parameters: [
      { symbol: 'ρ₀', label: 'Liquid density (1000 °C)', value: '6500', unit: 'kg/m³' },
      { symbol: 'ν_Sn', label: 'Kinematic viscosity', value: '2.80 × 10⁻⁷', unit: 'm²/s' },
      { symbol: 'β_Sn', label: 'Thermal expansion coeff.', value: '9.09 × 10⁻⁵', unit: 'K⁻¹' },
      { symbol: 'k_Sn', label: 'Thermal conductivity', value: '32.0', unit: 'W/(m·K)' },
      { symbol: 'σ_elec', label: 'Electrical conductivity', value: '2.10 × 10⁶', unit: 'S/m' },
    ],
    fsmMaterial: {
      id: 'molten_tin',
      name: 'Molten Tin Bath Sn(l)',
      e: 1200.0,
      nu: 0.45,
      yield_strength: 18.0,
      density: 6.5,
      min_wall: 0.8,
      alpha: 90.9e-6,
      source:
        'Pilkington UK5 tin2fe liquid-metal reference (Ron Smith, J. Fluid Mech. 1990, 214, 211–228): ρ₀ = 6500 kg/m³, β_Sn = 9.09×10⁻⁵ K⁻¹, k = 32 W/(m·K).',
    },
  },
  {
    id: 'soda_lime_glass',
    name: 'Soda-Lime Float Glass (VFT Viscous Melt)',
    phase: 'viscous_melt',
    phaseLabel: 'VFT Viscous Melt',
    badgeColor: '#f59e0b',
    defaultHexColor: '#38bdf8',
    constitutiveEquation:
      'log₁₀ η(T) = −A + B / (T − T₀),   σ_nn = γ_g / h − ½ ρ_g g (1 − ρ_g / ρ_Sn) h',
    parameters: [
      { symbol: 'A, B, T₀', label: 'VFT viscosity constants', value: '2.65, 4210, 264', unit: '—, K, °C' },
      { symbol: 'η(1050 °C)', label: 'Spout entry viscosity', value: '1.25 × 10³', unit: 'Pa·s' },
      { symbol: 'η(600 °C)', label: 'Lehr exit viscosity', value: '1.60 × 10¹⁰', unit: 'Pa·s' },
      { symbol: 'γ_g', label: 'Surface tension', value: '0.30', unit: 'N/m' },
      { symbol: 'h_eq', label: 'Capillary–gravity eq. thickness', value: '6.84', unit: 'mm' },
      { symbol: 'ρ_g', label: 'Melt density', value: '2500', unit: 'kg/m³' },
    ],
    fsmMaterial: {
      id: 'soda_lime_glass',
      name: 'Soda-Lime Float Glass (VFT Melt)',
      e: 72000.0,
      nu: 0.22,
      yield_strength: 65.0,
      density: 2.5,
      min_wall: 0.8,
      alpha: 9.0e-6,
      source:
        'Pilkington UK5 float2fe soda-lime-silica VFT rheology: log₁₀ η(T) = -2.65 + 4210/(T - 264 °C), ρ_g = 2.50 g/cm³, E_solid = 72 GPa, CTE = 9.0×10⁻⁶/°C.',
    },
  },
  {
    id: 'sic_refractory',
    name: 'Alumino-Silicate / SiC Bath Casing Refractory',
    phase: 'solid_elastic',
    phaseLabel: 'Refractory Thermo-Elastic Solid',
    badgeColor: '#3b82f6',
    defaultHexColor: '#6b665e',
    constitutiveEquation:
      'σ = C : (ε(u) − α_ref (T − T_ref) I),   q = −k_ref ∇T',
    parameters: [
      { symbol: 'E', label: 'Young’s modulus', value: '95.0', unit: 'GPa' },
      { symbol: 'ν', label: 'Poisson’s ratio', value: '0.20', unit: '—' },
      { symbol: 'α_ref', label: 'Thermal expansion coeff.', value: '4.60 × 10⁻⁶', unit: 'K⁻¹' },
      { symbol: 'k_ref', label: 'Thermal conductivity', value: '2.40', unit: 'W/(m·K)' },
      { symbol: 'ρ', label: 'Bulk density', value: '2650', unit: 'kg/m³' },
    ],
    fsmMaterial: {
      id: 'sic_refractory',
      name: 'Alumino-Silicate / SiC Refractory',
      e: 95000.0,
      nu: 0.20,
      yield_strength: 140.0,
      density: 2.65,
      min_wall: 1.5,
      alpha: 4.6e-6,
      source:
        'Fused-cast alumino-silicate / SiC float-bath lip and bottom block refractory: E = 95 GPa, CTE = 4.6×10⁻⁶/°C, ρ = 2.65 g/cm³.',
    },
  },
  {
    id: 'h2_n2_gas',
    name: 'Reducing H₂/N₂ Bath Atmosphere & Redox Cushion',
    phase: 'reactive_gas',
    phaseLabel: 'Reducing Gas · Navier-Slip Cushion',
    badgeColor: '#10b981',
    defaultHexColor: '#a7f3d0',
    constitutiveEquation:
      'L_s = (μ_Sn / μ_gas) δ_film(X_H2, T),   q_int = (k_gas / δ_film) (T_g − T_Sn)',
    parameters: [
      { symbol: 'X_H2', label: 'Hydrogen volume fraction', value: '4.2 – 10.0', unit: '%' },
      { symbol: 'k_gas', label: 'Gas thermal conductivity', value: '0.095', unit: 'W/(m·K)' },
      { symbol: 'L_s', label: 'Navier slip length at contact', value: '40 – 150', unit: 'μm' },
      { symbol: 'δ_film', label: 'Redox gas cushion thickness', value: '1.2 – 4.8', unit: 'μm' },
    ],
    fsmMaterial: {
      id: 'h2_n2_gas',
      name: 'Reducing H₂/N₂ Bath Atmosphere',
      e: 100.0,
      nu: 0.30,
      yield_strength: 5.0,
      density: 0.15,
      min_wall: 0.8,
      alpha: 15.0e-6,
      source:
        'Forming-gas reducing headspace (4–10% H₂ in N₂) + interfacial SnO/H₂ redox cushion (L_s = (μ_Sn/μ_gas) δ_film).',
    },
  },
  {
    id: 'nbk7_glass',
    name: 'N-BK7 Optical Crown Glass (Refractive Lens)',
    phase: 'optical_medium',
    phaseLabel: 'Optical Dielectric · Snell/Fresnel',
    badgeColor: '#38bdf8',
    defaultHexColor: '#7dd3fc',
    constitutiveEquation:
      'n₁ sin θ₁ = n₂ sin θ₂,   R_F = ((n − 1)/(n + 1))²,   τ_lens(r) = τ₀ exp(−K_ext δ_soot(r))',
    parameters: [
      { symbol: 'n_d', label: 'Refractive index (587.6 nm)', value: '1.5168', unit: '—' },
      { symbol: 'V_d', label: 'Abbe dispersion number', value: '64.17', unit: '—' },
      { symbol: 'τ₀', label: 'Clean solar transmittance (2 surfaces)', value: '91.8', unit: '%' },
      { symbol: 'K_ext', label: 'Soot Mie extinction coeff.', value: '2.40', unit: 'μm⁻¹' },
      { symbol: 'E', label: 'Elastic modulus', value: '82.0', unit: 'GPa' },
      { symbol: 'α', label: 'Thermal expansion coeff.', value: '7.10 × 10⁻⁶', unit: 'K⁻¹' },
    ],
    fsmMaterial: {
      id: 'nbk7_glass',
      name: 'N-BK7 Optical Crown Glass (n=1.517)',
      e: 82000.0,
      nu: 0.206,
      yield_strength: 70.0,
      density: 2.51,
      min_wall: 0.8,
      alpha: 7.1e-6,
      source:
        'Schott N-BK7 optical borosilicate crown glass data sheet: n_d = 1.5168, V_d = 64.17, E = 82 GPa, ν = 0.206, ρ = 2.51 g/cm³, CTE = 7.1×10⁻⁶/°C.',
    },
  },
  {
    id: 'paraffin_wax',
    name: 'Paraffin Candle Wax C₂₅H₅₂ (Stefan Phase-Change Fuel)',
    phase: 'phase_change_fuel',
    phaseLabel: 'Phase-Change Fuel · Stefan/Pyrolysis',
    badgeColor: '#f97316',
    defaultHexColor: '#fde68a',
    constitutiveEquation:
      'ρ_w c_eff(T) ∂T/∂t = ∇·(k_w ∇T) − L_f ρ_w ∂f_L/∂t,   ṁ_pyr = A_p ρ_w exp(−E_p / R T)',
    parameters: [
      { symbol: 'T_melt', label: 'Solid–liquid melting point', value: '58.0', unit: '°C' },
      { symbol: 'L_f', label: 'Latent heat of fusion', value: '210.0', unit: 'kJ/kg' },
      { symbol: 'T_pyr', label: 'Vapor pyrolysis onset', value: '360.0', unit: '°C' },
      { symbol: 'ΔH_comb', label: 'Lower heating value (C₂₅H₅₂)', value: '42.0', unit: 'MJ/kg' },
      { symbol: 'k_w', label: 'Thermal conductivity', value: '0.25', unit: 'W/(m·K)' },
      { symbol: 'ρ_w', label: 'Solid density', value: '900', unit: 'kg/m³' },
    ],
    fsmMaterial: {
      id: 'paraffin_wax',
      name: 'Paraffin Candle Wax C₂₅H₅₂',
      e: 450.0,
      nu: 0.38,
      yield_strength: 12.0,
      density: 0.9,
      min_wall: 0.8,
      alpha: 110.0e-6,
      source:
        'Paraffin wax (n-pentacosane C₂₅H₅₂): T_melt = 58 °C, L_f = 210 kJ/kg, T_pyrolysis = 360 °C, ΔH_comb = 42.0 MJ/kg, ρ = 0.90 g/cm³.',
    },
  },
  {
    id: 'cotton_wick',
    name: 'Braided Cotton Capillary Wick (Darcy + Arrhenius)',
    phase: 'phase_change_fuel',
    phaseLabel: 'Capillary Wick · Darcy + Ignition',
    badgeColor: '#ea580c',
    defaultHexColor: '#78350f',
    constitutiveEquation:
      'u_wick = −(K_w / μ_wax) (∇p − ρ_w g + 2 γ_w cos θ / r_p),   ω̇_comb = A ρ² Y_F Y_O2 exp(−T_a / T)',
    parameters: [
      { symbol: 'K_w', label: 'Darcy permeability', value: '4.50 × 10⁻¹¹', unit: 'm²' },
      { symbol: 'r_p', label: 'Effective capillary pore radius', value: '18.0', unit: 'μm' },
      { symbol: 'α_abs', label: 'Solar absorptivity (charred tip)', value: '0.88', unit: '—' },
      { symbol: 'T_a', label: 'Arrhenius activation temp. E_a/R', value: '14500', unit: 'K' },
    ],
    fsmMaterial: {
      id: 'cotton_wick',
      name: 'Braided Cotton Capillary Wick',
      e: 900.0,
      nu: 0.32,
      yield_strength: 25.0,
      density: 0.55,
      min_wall: 0.6,
      alpha: 25.0e-6,
      source:
        'Porous braided cellulose capillary wick: Darcy permeability K_w = 4.5×10⁻¹¹ m², pore radius r_p = 18 μm, charred absorptivity α = 0.88.',
    },
  },
  {
    id: 'air_gas',
    name: 'Buoyant Air / Plume Continuum (O₂/N₂ + Soot Transport)',
    phase: 'reactive_gas',
    phaseLabel: 'Buoyant Plume · Low-Mach NS',
    badgeColor: '#ef4444',
    defaultHexColor: '#94a3b8',
    constitutiveEquation:
      'ρ ∂u/∂t + ρ (u·∇)u = −∇p + ∇·(μ ∇u) + (ρ_∞ − ρ(T)) g ẑ,   v_th = −C_th (ν / T) ∇T',
    parameters: [
      { symbol: 'ρ_∞', label: 'Ambient air density (20 °C)', value: '1.20', unit: 'kg/m³' },
      { symbol: 'Y_O2,∞', label: 'Ambient oxygen mass fraction', value: '0.232', unit: 'kg/kg' },
      { symbol: 'Pr, Sc', label: 'Prandtl / Schmidt numbers', value: '0.71, 0.70', unit: '—' },
      { symbol: 'C_th', label: 'Rosner thermophoretic coeff.', value: '0.55', unit: '—' },
    ],
    fsmMaterial: {
      id: 'air_gas',
      name: 'Buoyant Air / Plume Continuum',
      e: 120.0,
      nu: 0.30,
      yield_strength: 5.0,
      density: 0.12,
      min_wall: 0.8,
      alpha: 20.0e-6,
      source:
        'Standard dry air oxidizer continuum (23.2 wt% O₂, Pr = 0.71, Sc = 0.70, Rosner thermophoresis C_th = 0.55).',
    },
  },
  {
    id: 'jet_a1_spray',
    name: 'Jet-A1 Kerosene Swirl Spray (Euler–Lagrange Combustion)',
    phase: 'reactive_gas',
    phaseLabel: 'Swirl Spray · Reacting Flow',
    badgeColor: '#dc2626',
    defaultHexColor: '#fb923c',
    constitutiveEquation:
      'd(d_p²)/dt = −K_evap(Re_p, Bm),   ω̇_F = A_sw ρ² Y_F Y_O2 exp(−T_a / T)',
    parameters: [
      { symbol: 'd₃₂', label: 'Sauter mean droplet diameter', value: '35.0', unit: 'μm' },
      { symbol: 'L_v', label: 'Latent heat of vaporization', value: '290.0', unit: 'kJ/kg' },
      { symbol: 'ΔH_comb', label: 'Lower heating value (Jet-A1)', value: '43.2', unit: 'MJ/kg' },
      { symbol: 'S_w', label: 'Nominal vane swirl number', value: '0.72', unit: '—' },
    ],
    fsmMaterial: {
      id: 'jet_a1_spray',
      name: 'Jet-A1 Kerosene Swirl Spray',
      e: 800.0,
      nu: 0.35,
      yield_strength: 20.0,
      density: 0.81,
      min_wall: 0.6,
      alpha: 45.0e-6,
      source:
        'Aviation kerosene (Jet-A1 / C₁₂H₂₃) hollow-cone swirl spray: LHV = 43.2 MJ/kg, d₃₂ = 35 μm, L_v = 290 kJ/kg.',
    },
  },
  {
    id: 'in718',
    name: 'Inconel 718 Nickel Superalloy (LPBF)',
    phase: 'solid_elastic',
    phaseLabel: 'High-Temp Superalloy Solid',
    badgeColor: '#3b82f6',
    defaultHexColor: '#64748b',
    constitutiveEquation:
      'σ = C : (ε(u) − α_IN718 (T − T_ref) I),   q = −k_IN718 ∇T',
    parameters: [
      { symbol: 'E', label: 'Young’s modulus', value: '190.0', unit: 'GPa' },
      { symbol: 'R_p0.2', label: '0.2% proof yield strength', value: '980.0', unit: 'MPa' },
      { symbol: 'α', label: 'Thermal expansion coeff.', value: '13.0 × 10⁻⁶', unit: 'K⁻¹' },
      { symbol: 'k', label: 'Thermal conductivity', value: '11.4', unit: 'W/(m·K)' },
      { symbol: 'ρ', label: 'Density', value: '8190', unit: 'kg/m³' },
    ],
    fsmMaterial: {
      id: 'in718',
      name: 'Inconel 718 (LPBF)',
      e: 190000.0,
      nu: 0.29,
      yield_strength: 980.0,
      density: 8.19,
      min_wall: 0.4,
      alpha: 13.0e-6,
      source:
        'EOS NickelAlloy IN718 heat-treated LPBF data sheet: E = 190 GPa, Rp0.2 = 980 MPa, ρ = 8.19 g/cm³, CTE = 13.0×10⁻⁶/°C.',
    },
  },
  {
    id: 'cf_woven_3k',
    name: '3K Woven CFRP Laminate (Space Epoxy, Tsai–Wu)',
    phase: 'solid_elastic',
    phaseLabel: 'Orthotropic Woven Composite · Tsai–Wu',
    badgeColor: '#0284c7',
    defaultHexColor: '#334155',
    constitutiveEquation:
      'σ = C_ortho(E₁₁, E₂₂, G₁₂) : ε,   U_peel = √((max(0, σ_nn)/S_z)² + (τ_nt/S_s)²) ≤ 1',
    parameters: [
      { symbol: 'E₁₁ = E₂₂', label: 'In-plane woven modulus', value: '70.0', unit: 'GPa' },
      { symbol: 'S_inline', label: 'In-plane tensile/bearing strength', value: '620.0', unit: 'MPa' },
      { symbol: 'S_z', label: 'Interlaminar tensile peel strength', value: '42.0', unit: 'MPa' },
      { symbol: 'S_s', label: 'Interlaminar shear strength (ASTM D2344)', value: '58.0', unit: 'MPa' },
      { symbol: 'α', label: 'In-plane thermal expansion', value: '2.1 × 10⁻⁶', unit: 'K⁻¹' },
    ],
    fsmMaterial: {
      id: 'cf_woven_3k',
      name: '3K Woven CFRP Laminate (Space Epoxy, Tsai–Wu)',
      e: 70000.0,
      nu: 0.1,
      yield_strength: 620.0,
      density: 1.58,
      min_wall: 0.6,
      alpha: 2.1e-6,
      yield_z: 42.0,
      shear_z: 58.0,
      source:
        'Aerospace 3K plain-weave carbon/epoxy quasi-balanced laminate: E₁₁=E₂₂=70 GPa, S_z=42 MPa, S_s=58 MPa.',
    },
  },
  {
    id: 'sic_jwst',
    name: 'SiC Space-Optics Ceramic (Weibull Brittle)',
    phase: 'solid_elastic',
    phaseLabel: 'Cryo Space-Optics Ceramic · Weibull',
    badgeColor: '#0ea5e9',
    defaultHexColor: '#475569',
    constitutiveEquation:
      'P_f(σ₁) = 1 − exp(−∫_V (max(0, σ₁) / σ₀)^m dV / V₀)',
    parameters: [
      { symbol: 'E', label: 'Young’s modulus', value: '410.0', unit: 'GPa' },
      { symbol: 'σ₀', label: 'Weibull characteristic scale (63.2%)', value: '340.0', unit: 'MPa' },
      { symbol: 'm', label: 'Weibull flaw modulus', value: '10.5', unit: '—' },
      { symbol: 'α', label: 'Cryogenic CTE', value: '2.2 × 10⁻⁶', unit: 'K⁻¹' },
    ],
    fsmMaterial: {
      id: 'sic_jwst',
      name: 'SiC Space-Optics Ceramic (Weibull Brittle)',
      e: 410000.0,
      nu: 0.17,
      yield_strength: 340.0,
      density: 3.1,
      min_wall: 1.0,
      alpha: 2.2e-6,
      source:
        'Sintered Silicon Carbide (Boostec SiC space-optics bench class): E = 410 GPa, ν = 0.17, σ₀ = 340 MPa, m = 10.5.',
    },
  },
];

/** Look up rich continuum metadata for any material ID (built-in solid, multiphysics continuum, or custom). */
export function getContinuumMaterialSpec(doc: Document, matId: string): ContinuumMaterialSpec {
  const mp = MULTIPHYSICS_CONTINUUM_MATERIALS.find((m) => m.id === matId);
  if (mp) return mp;

  const allLib = [...materialLibrary(), ...(doc.custom_materials ?? [])];
  const found = allLib.find((m) => m.id === matId);
  const name = found?.name ?? matId;
  const eGpa = found ? (found.e / 1000).toFixed(1) : '—';
  const nu = found ? found.nu.toFixed(2) : '—';
  const ys = found ? found.yield_strength.toFixed(0) : '—';
  const rho = found ? (found.density * 1000).toFixed(0) : '—';
  const alphaVal = (found as unknown as { alpha?: number })?.alpha;
  const cte = alphaVal != null ? `${(alphaVal * 1e6).toFixed(1)} × 10⁻⁶` : '15.0 × 10⁻⁶';

  return {
    id: matId,
    name,
    phase: 'solid_elastic',
    phaseLabel: 'Solid Elastic / Thermo-Elastic',
    badgeColor: '#3b82f6',
    defaultHexColor: '#94a3b8',
    constitutiveEquation: 'σ = C(E, ν) : (ε(u) − α (T − T_ref) I),   q = −k ∇T',
    parameters: [
      { symbol: 'E', label: 'Young’s modulus', value: eGpa, unit: 'GPa' },
      { symbol: 'ν', label: 'Poisson’s ratio', value: nu, unit: '—' },
      { symbol: 'σ_y', label: 'Yield strength', value: ys, unit: 'MPa' },
      { symbol: 'ρ', label: 'Density', value: rho, unit: 'kg/m³' },
      { symbol: 'α', label: 'Thermal expansion (CTE)', value: cte, unit: 'K⁻¹' },
    ],
    fsmMaterial: found ?? {
      id: matId,
      name,
      e: 70000,
      nu: 0.33,
      yield_strength: 230,
      density: 2.67,
      min_wall: 0.8,
      source: 'Built-in solid library',
    },
  };
}

/** Ensure that if `matId` is one of the multiphysics continuum materials, its `Material` entry
 * is present in `draft.custom_materials` so WASM `parseFsm`/`printFsm`/`analyse` work natively. */
export function ensureContinuumMaterialInDoc(draft: Document, matId: string): void {
  const mp = MULTIPHYSICS_CONTINUUM_MATERIALS.find((m) => m.id === matId);
  if (!mp) return;
  if (!draft.custom_materials) draft.custom_materials = [];
  if (!draft.custom_materials.some((m) => m.id === matId)) {
    draft.custom_materials.push(structuredClone(mp.fsmMaterial));
  }
}

/** Set or update a named scalar `let` parameter in `draft.lets`, and re-evaluate any
 * consumer expressions in `draft.consumed` that depend on it. */
export function upsertLetParam(draft: Document, name: string, value: number): void {
  if (!draft.lets) draft.lets = [];
  const existing = draft.lets.find((l) => l.name === name);
  const formatted = Number.isInteger(value) ? String(value) : String(Number(value.toFixed(4)));
  if (existing) {
    existing.value = value;
    existing.expr = formatted;
  } else {
    draft.lets.push({ name, expr: formatted, value });
  }
  if (draft.consumed && draft.consumed.length > 0) {
    try {
      applyAddrs(draft, [[{ id: name, attr: '', comp: null }, value]], true);
    } catch {
      // Ignore if an unrelated consumer expression is mid-edit
    }
  }
}

/** Read a named scalar `let` parameter from `doc.lets`, or return `fallback`. */
export function readLetParam(doc: Document, name: string, fallback: number): number {
  const found = doc.lets?.find((l) => l.name === name);
  return found && Number.isFinite(found.value) ? found.value : fallback;
}

// ---- Active Field Equation Detection & Coupled Regime Resolution ----------------------

export type CoupledPhysicsRegime =
  | 'pilkington_uk5_float_bath'
  | 'solar_candle_ignition'
  | 'annular_swirl_spray_combustor'
  | null;

export type DesignStageId = 'shape' | 'setup' | 'results' | 'fill' | 'make';

export interface ActiveFieldEquation {
  id: string;
  stage: DesignStageId;
  stageNumber: number;
  title: string;
  shortName: string;
  active: boolean;
  /** Exact reason this equation is active (or what in Shape/Setup would activate it). */
  triggerSummary: string;
  /** Strong differential form in clean terminal-friendly Unicode. */
  strongForm: string;
  /** Weak variational / discretized finite-element or finite-volume form. */
  weakOrDiscreteForm: string;
  /** Live substituted parameters from the user's current document. */
  liveParameters: string[];
}

/** Collect all volume nodes and their effective material + continuum spec. */
export function collectVolumeContinuumNodes(doc: Document): {
  node: Node;
  matId: string;
  spec: ContinuumMaterialSpec;
}[] {
  const out: { node: Node; matId: string; spec: ContinuumMaterialSpec }[] = [];
  walk(doc.root, (n) => {
    if (n.kind.type === 'group' || n.suppressed) return;
    const matId = n.material || doc.material;
    out.push({
      node: n,
      matId,
      spec: getContinuumMaterialSpec(doc, matId),
    });
  });
  return out;
}

/** Determine which coupled multiphysics benchmark digital-twin regime is active.
 * IMPORTANT: Only return one of the 3 fixed-coordinate benchmark twin regimes when the
 * document actually IS that multi-body benchmark assembly (by volume material pair,
 * benchmark node IDs, or explicit benchmark document name). Never trigger a benchmark
 * 3D overlay merely because a user added a general `let` parameter or boundary condition
 * in Setup (2) on their own CAD part. */
export function detectCoupledPhysicsRegime(doc: Document): CoupledPhysicsRegime {
  const vols = collectVolumeContinuumNodes(doc);
  const matIds = new Set<string>([doc.material, ...vols.map((v) => v.matId)]);
  const nodeIds = new Set<string>(vols.map((v) => v.node.id.toLowerCase()));
  const nameLower = (doc.name ?? '').toLowerCase();

  // 1. Pilkington UK5 Float-Bath benchmark twin: requires both molten_tin and soda_lime_glass volumes,
  //    or explicit float-bath assembly node IDs / document title.
  if (
    (matIds.has('molten_tin') && matIds.has('soda_lime_glass')) ||
    (nodeIds.has('tin_bath_pool') && nodeIds.has('glass_ribbon')) ||
    nameLower.includes('float-bath') ||
    nameLower.includes('float bath') ||
    nameLower.includes('pilkington') ||
    nameLower.includes('uk5') ||
    nameLower.includes('t8 —')
  ) {
    return 'pilkington_uk5_float_bath';
  }

  // 2. Solar Candle Ignition benchmark twin: requires paraffin_wax + cotton_wick volumes,
  //    or explicit candle assembly node IDs / document title.
  if (
    (matIds.has('paraffin_wax') && matIds.has('cotton_wick')) ||
    (nodeIds.has('candle_column') && nodeIds.has('candle_wick')) ||
    (nodeIds.has('focus_lens') && nodeIds.has('wax_column')) ||
    nameLower.includes('candle') ||
    nameLower.includes('t13 —')
  ) {
    return 'solar_candle_ignition';
  }

  // 3. Annular Swirl Spray Combustor benchmark twin: requires combustor + swirl chamber nodes,
  //    or explicit swirl combustor document title.
  if (
    (nodeIds.has('combustor_casing') && nodeIds.has('swirl_chamber')) ||
    (matIds.has('jet_a1_spray') && nodeIds.has('nozzle_body')) ||
    nameLower.includes('swirl atomiser') ||
    nameLower.includes('spray combustor') ||
    nameLower.includes('t9 —')
  ) {
    return 'annular_swirl_spray_combustor';
  }

  return null;
}

/** Inspect the live Document and return the complete list of Field Equations (both active and standby)
 * with exact triggers and live parameter substitutions. */
export function evaluateDocumentFieldEquations(doc: Document): ActiveFieldEquation[] {
  const vols = collectVolumeContinuumNodes(doc);
  const phases = new Set<ContinuumPhaseId>(vols.map((v) => v.spec.phase));
  const matIds = new Set<string>([doc.material, ...vols.map((v) => v.matId)]);
  const letNames = new Set<string>((doc.lets ?? []).map((l) => l.name));
  const regime = detectCoupledPhysicsRegime(doc);

  const hasShapes = doc.root.length > 0;
  const hasSupports = doc.supports.length > 0;
  const hasMechLoads = doc.loads.length > 0;
  const hasThermalBcs =
    doc.temperatures.length > 0 ||
    doc.convections.length > 0 ||
    doc.heat_fluxes.length > 0;
  const hasElasticSolve = hasSupports && (hasMechLoads || hasThermalBcs);
  const hasSubdomains = vols.some((v) => v.matId !== doc.material);
  const hasModal = doc.solve.modes != null && doc.solve.modes > 0;
  const hasFill = doc.fill != null;
  const hasErode = (doc.fill?.erode ?? 0) > 0;

  const isViscousCreep =
    regime === 'pilkington_uk5_float_bath' ||
    phases.has('viscous_melt') ||
    letNames.has('u_pull') ||
    letNames.has('h_sill') ||
    doc.loads.some((l) => l.id.includes('viscous_pull'));
  const isEmLorentz =
    regime === 'pilkington_uk5_float_bath' ||
    phases.has('liquid_metal') ||
    letNames.has('i_current') ||
    letNames.has('b_field') ||
    letNames.has('i_motor') ||
    doc.loads.some((l) => l.id.includes('lorentz')) ||
    doc.heat_fluxes.some((f) => f.id.includes('joule'));
  const isOptics =
    regime === 'solar_candle_ignition' ||
    phases.has('optical_medium') ||
    letNames.has('i_beam') ||
    letNames.has('i_solar') ||
    doc.heat_fluxes.some((f) => f.id.includes('optical') || f.id.includes('solar'));
  const isPhaseChange =
    regime === 'solar_candle_ignition' || phases.has('phase_change_fuel');
  const isReactiveFlow =
    regime === 'solar_candle_ignition' ||
    regime === 'annular_swirl_spray_combustor' ||
    phases.has('reactive_gas') ||
    phases.has('phase_change_fuel') ||
    letNames.has('swirl_number') ||
    letNames.has('m_dot_jet') ||
    letNames.has('m_dot_fuel') ||
    doc.temperatures.some((t) => t.id.includes('flame') || t.id.includes('thermal_jet'));

  const isDroneAntenna =
    letNames.has('cfrp_standoff_mm') ||
    letNames.has('gemini_l900_mm') ||
    letNames.has('gemini_l2400_mm') ||
    doc.name.toLowerCase().includes('deadcat') ||
    doc.name.toLowerCase().includes('antenna');

  const docMatSpec = getContinuumMaterialSpec(doc, doc.material);

  return [
    // 1. Shape Stage: Implicit CSG & Subdomain Indicator Calculus
    {
      id: 'implicit_csg_sdf',
      stage: 'shape',
      stageNumber: 1,
      title: 'Implicit Signed Distance Field (SDF) & R-Function CSG Calculus',
      shortName: 'Implicit SDF & CSG Calculus',
      active: hasShapes,
      triggerSummary: hasShapes
        ? `Active: ${vols.length} primitive/group volume(s) in Shape (1)${hasSubdomains ? ` across ${matIds.size} distinct material subdomains` : ''}.`
        : 'Standby: Add a shape primitive (Box, Cylinder, Extrude, Revolve, …) in Shape (1).',
      strongForm:
        'd_Ω(x) = fold_k Op_k(d_{k−1}(x), T_k⁻¹ d_prim,k(x), r_blend),   Ω = { x ∈ ℝ³ : d_Ω(x) ≤ 0 }',
      weakOrDiscreteForm:
        'smin_r(a, b) = min(a, b) − (1 / 6r²) max(r − |a − b|, 0)³,   χ_m(x) = H(−d_{Ω_m}(x))',
      liveParameters: [
        `Active CSG primitives: ${vols.map((v) => `${v.node.id} (${v.node.op}, ${v.spec.name})`).join(', ') || 'none'}`,
        `Subdomain indicator fields χ_m(x): ${matIds.size} material domain(s) (${Array.from(matIds).join(', ')})`,
      ],
    },

    // 1b. Shape Stage: Parametric 2D Sketch Constraint Solver (Damped Levenberg–Marquardt + Column-Pivoted QR + Surface-Attached 3D Extrude/Revolve)
    {
      id: 'sketch_lm_qr_constraint_solver',
      stage: 'shape',
      stageNumber: 1,
      title: 'Parametric 2D Sketch Constraint Solver (Damped Levenberg–Marquardt, Homotopy & Column-Pivoted QR Rank)',
      shortName: 'Parametric Sketch Solver (LM + QR DOF & Revolve SDF)',
      active: vols.some((v) => v.node.kind.type === 'extrude' || v.node.kind.type === 'revolve'),
      triggerSummary: vols.some((v) => v.node.kind.type === 'extrude' || v.node.kind.type === 'revolve')
        ? `Active: ${vols.filter((v) => v.node.kind.type === 'extrude' || v.node.kind.type === 'revolve').length} profile-driven Extrude/Revolve feature(s) in Shape (1).`
        : 'Standby: Click ✏️ Sketch… in Shape (1) or add an Extrude / Revolve profile.',
      strongForm:
        '(Jᵀ J + λ I) δq = −Jᵀ r(q; α),   DOF = N_vars − rank(J),   d_revolve(x) = d_poly2d(√(x² + y²), z)',
      weakOrDiscreteForm:
        'J P = Q R  (Householder column-pivoted QR, |R_kk| > 10⁻⁷)  ⇒  P_null = I − J⁺ J,   homotopy α ∈ {0.25, 0.5, 0.75, 1.0}',
      liveParameters: [
        `Profile-driven 3D nodes: ${vols.filter((v) => v.node.kind.type === 'extrude' || v.node.kind.type === 'revolve').map((v) => `${v.node.id} (${v.node.kind.type})`).join(', ') || 'none (ready in ✏️ Sketch…)'}`,
        `Parametric let variables available for sketch dimensions: ${(doc.lets ?? []).map((l) => `${l.name}=${l.value}`).join(', ') || 'none'}`,
      ],
    },

    // 2. Setup / Results Stage: 3D Quadratic (P2) Linear Elasticity
    {
      id: 'p2_linear_elasticity',
      stage: 'setup',
      stageNumber: 2,
      title: '3D Quadratic (P2) Navier–Cauchy Linear Elasticity',
      shortName: '3D P2 Linear Elasticity (Ku = f)',
      active: hasElasticSolve,
      triggerSummary: hasElasticSolve
        ? `Active: ${doc.supports.length} support(s) (${doc.supports.map((s) => s.id).join(', ')}) + ${doc.loads.length} mechanical load(s) / ${doc.temperatures.length + doc.convections.length + doc.heat_fluxes.length} thermal BC(s) in Setup (2).`
        : 'Standby: Add at least one Support and one Load or Thermal BC in Setup (2).',
      strongForm:
        '−∇ · σ(u) = f_b  in Ω,   σ(u) = C_e : ε(u),   ε(u) = ½ (∇u + ∇uᵀ),   u = 0 on Γ_D,   σ · n = t̂ on Γ_N',
      weakOrDiscreteForm:
        'Find u_h ∈ V_h^(P2) : ∫_Ω ε(v_h) : C_e : ε(u_h) dΩ = ∫_{Γ_N} v_h · t̂ dΓ + l_th(v_h),   10-node P2 tet + Keast deg-2 quadrature',
      liveParameters: [
        `Base continuum material: ${docMatSpec.name} (E = ${(docMatSpec.fsmMaterial.e / 1000).toFixed(1)} GPa, ν = ${docMatSpec.fsmMaterial.nu})`,
        `Dirichlet supports Γ_D: ${doc.supports.map((s) => `${s.id} on ${s.region} [fixed=${['x', 'y', 'z'].filter((_, i) => s.fixed[i]).join('')}]`).join('; ') || 'none'}`,
        `Neumann tractions Γ_N: ${
          doc.loads
            .map((l) =>
              l.kind.type === 'force'
                ? `${l.id} on ${l.region} (F = [${l.kind.vector.join(', ')}] N)`
                : l.kind.type === 'pressure'
                  ? `${l.id} on ${l.region} (p = ${l.kind.mpa} MPa)`
                  : l.kind.type === 'preload'
                    ? `${l.id} on ${l.region} (F_preload = ${l.kind.newtons} N)`
                    : `${l.id} on ${l.region} (p(x,y,z) = ${l.kind.expr} MPa)`,
            )
            .join('; ') || (hasThermalBcs ? 'thermal-driven' : 'none')
        }`,
      ],
    },

    // 3. Setup / Results Stage: Steady-State Fourier Heat Conduction & Duhamel–Neumann Thermo-Elasticity
    {
      id: 'fourier_thermo_elasticity',
      stage: 'setup',
      stageNumber: 2,
      title: 'Fourier Heat Conduction & Duhamel–Neumann Thermo-Elastic Coupling',
      shortName: 'Fourier Conduction & Thermo-Elasticity',
      active: hasThermalBcs,
      triggerSummary: hasThermalBcs
        ? `Active: ${doc.temperatures.length} Dirichlet T, ${doc.convections.length} Robin convection, and ${doc.heat_fluxes.length} Neumann heat-flux BC(s) in Setup (2).`
        : 'Standby: Attach a Temperature, Convection, or Heat flux boundary condition to any region in Setup (2).',
      strongForm:
        '−∇ · (k_e ∇T) = 0  in Ω,   T = T_D on Γ_T,   −k_e ∇T · n = h_c (T − T_∞) + q_N on Γ_R ∪ Γ_q,   ε_th = α_e (T − T_ref) I',
      weakOrDiscreteForm:
        '∫_Ω k_e ∇w_h · ∇T_h dΩ + ∫_{Γ_R} h_c w_h T_h dΓ = ∫_{Γ_R} h_c w_h T_∞ dΓ − ∫_{Γ_q} w_h q_N dΓ  →  f_th = ∫_Ω Bᵀ C_e α_e ΔT_h dΩ',
      liveParameters: hasThermalBcs
        ? [
            `Reference stress-free temperature T_ref = ${doc.solve.t_ref ?? 20} °C`,
            `Dirichlet temperatures Γ_T: ${doc.temperatures.map((t) => `${t.id} (${t.region}: ${t.celsius} °C)`).join(', ') || 'none'}`,
            `Robin convection Γ_R: ${doc.convections.map((c) => `${c.id} (${c.region}: h=${c.h}, T_∞=${c.t_inf} °C)`).join(', ') || 'none'}`,
            `Neumann heat flux Γ_q: ${doc.heat_fluxes.map((q) => `${q.id} (${q.region}: q=${q.q})`).join(', ') || 'none'}`,
          ]
        : ['Standby operator — attach a thermal boundary condition in Setup (2) to bind live parameters.'],
    },

    // 4. Viscoelastic / VFT Melt Extension & Creep Rheology
    {
      id: 'vft_viscous_glass_sheet',
      stage: 'setup',
      stageNumber: 2,
      title:
        regime === 'pilkington_uk5_float_bath'
          ? '2D Extensional Viscous Glass Sheet & Vogel–Fulcher–Tammann (VFT) Rheology'
          : 'Viscoelastic / VFT Melt Extension & High-Temperature Creep Rheology',
      shortName:
        regime === 'pilkington_uk5_float_bath'
          ? 'VFT Viscous Glass Sheet (float2fe)'
          : 'VFT Viscous Melt & Creep Rheology',
      active: isViscousCreep,
      triggerSummary: isViscousCreep
        ? `Active: VFT Viscous Melt material in Shape (1) or Viscous Pull / Creep BC in Setup (2).`
        : "Standby: Assign a VFT Viscous Melt material in Shape (1) or click '+ Viscous Pull' in Setup (2).",
      strongForm:
        '∇ · (h η(T) [2 D + tr(D) I]) + τ_traction = 0,   log₁₀ η(T) = −A + B / (T − T₀)',
      weakOrDiscreteForm:
        'u_x ∂h/∂x + ∂(h u_y)/∂y = 0,   D = ½ (∇u + ∇uᵀ)',
      liveParameters: isViscousCreep
        ? [
            `VFT Constants: A = 2.65, B = 4210 K, T₀ = 264 °C (η spans 1.2×10³ Pa·s at 1050 °C → 1.6×10¹⁰ Pa·s at 600 °C)`,
            `Pull speed u_pull = ${readLetParam(doc, 'u_pull', 0.16).toFixed(2)} m/s`,
          ]
        : ['Standby operator — assign a VFT viscous melt material in Shape (1) to bind live parameters.'],
    },

    // 5. Electromagnetic Lorentz Force (J × B), Eddy-Current MHD & Joule Heating
    {
      id: 'turbulent_tin_vms_mhd',
      stage: 'setup',
      stageNumber: 2,
      title:
        regime === 'pilkington_uk5_float_bath'
          ? 'Depth-Averaged Turbulent Molten-Tin VMS Flow, Lorentz MHD & Redox Navier-Slip'
          : 'Electromagnetic Lorentz Force (J × B), Eddy-Current MHD & Joule Heating',
      shortName:
        regime === 'pilkington_uk5_float_bath'
          ? 'Molten-Tin VMS + Lorentz MHD + Navier Slip'
          : 'EM Lorentz Force (J × B) & Joule Heating',
      active: isEmLorentz,
      triggerSummary: isEmLorentz
        ? `Active: Electromagnetic Lorentz Force / MHD / Joule Heating BC in Setup (2) or conductive liquid-metal volume in Shape (1).`
        : "Standby: Click '+ EM Lorentz' in Setup (2) or assign a conductive MHD material in Shape (1).",
      strongForm:
        regime === 'pilkington_uk5_float_bath'
          ? '∂(H u_Sn)/∂t + ∇·(H u_Sn ⊗ u_Sn) + g H ∇ζ + ½ g H² β_Sn ∇T_Sn = (τ_ribbon − τ_bed)/ρ_Sn + ∇·(H ν_t ∇u_Sn) + F_EM(I_motor)'
          : 'f_L = J × B = (1 / μ₀) (∇ × B) × B,   −∇ · σ(u) = f_L  in Ω,   q_Joule = |J|² / σ_e',
      weakOrDiscreteForm:
        regime === 'pilkington_uk5_float_bath'
          ? 'Skew-symmetric Variational Multiscale (VMS) projection + interfacial Navier slip length L_s = (μ_Sn / μ_gas) δ_film(X_H2)'
          : '∫_Ω ε(v_h) : C_e : ε(u_h) dΩ = ∫_Ω v_h · (J × B) dΩ + ∫_{Γ_N} v_h · p_mag n̂ dΓ,   p_mag = B² / (2 μ₀)',
      liveParameters: !isEmLorentz
        ? ['Standby operator — assign a conductive MHD volume or Lorentz BC to bind live parameters.']
        : regime === 'pilkington_uk5_float_bath'
          ? [
              `Downstream sill depth H_sill = ${readLetParam(doc, 'h_sill', 65).toFixed(1)} mm`,
              `Linear induction motor current I_motor = ${readLetParam(doc, 'i_motor', 260).toFixed(0)} A`,
              `Reducing headspace hydrogen fraction X_H2 = ${readLetParam(doc, 'p_h2', 4.2).toFixed(2)} %, CVD beam load Q_beam = ${readLetParam(doc, 'q_beam', 34).toFixed(1)} kW`,
            ]
          : [
              `Excitation current I = ${readLetParam(doc, 'i_current', readLetParam(doc, 'i_motor', 250)).toFixed(0)} A, Magnetic flux density B₀ = ${readLetParam(doc, 'b_field', 0.8).toFixed(2)} T`,
              `Active Lorentz / Joule BCs: ${[...doc.loads.filter((l) => l.id.includes('lorentz')).map((l) => `${l.id} on ${l.region}`), ...doc.heat_fluxes.filter((f) => f.id.includes('joule')).map((f) => `${f.id} on ${f.region}`)].join(', ') || 'parameter coupled'}`,
            ],
    },

    // 6. Optical Snell–Descartes Ray Tracing & Caustic Irradiance Focusing
    {
      id: 'snell_fresnel_ray_optics',
      stage: 'setup',
      stageNumber: 2,
      title: 'Snell–Descartes Refractive Ray Tracing, Fresnel Losses & Caustic Focal Irradiance',
      shortName: 'Snell–Descartes Ray Optics & Radiative Flux',
      active: isOptics,
      triggerSummary: isOptics
        ? `Active: Optical lens volume ('nbk7_glass', n = 1.5168) in Shape (1) or Optical Ray Beam BC in Setup (2).`
        : "Standby: Assign an optical dielectric material in Shape (1) or click '+ Optical Ray Beam' in Setup (2).",
      strongForm:
        'n₁ (ŝ_i × n̂) = n₂ (ŝ_t × n̂),   1 / f = (n_lens − 1) (1 / R₁ − 1 / R₂),   I_focus(r, z) = τ_lens(t) I₀ (D_lens / 2 w(z))² exp(−2 r² / w(z)²)',
      weakOrDiscreteForm:
        'τ_lens(r, t) = (1 − R_Fresnel)² exp(−κ_abs d_lens) · exp(−K_ext δ_soot(r, t)),   K_ext = 2.4 μm⁻¹',
      liveParameters: isOptics
        ? [
            `Beam irradiance I₀ = ${readLetParam(doc, 'i_beam', readLetParam(doc, 'i_solar', 1000)).toFixed(0)} W/m², Lens aperture D_lens = ${readLetParam(doc, 'd_lens', 44).toFixed(1)} mm`,
            `Focal length f_lens = ${readLetParam(doc, 'f_lens', 60).toFixed(1)} mm, Refractive index n_d = 1.5168 (Fresnel loss 2×4.22%)`,
          ]
        : ['Standby operator — assign an optical refractive volume or ray beam BC to bind live parameters.'],
    },

    // 7. Stefan Phase-Change Melting, Darcy Capillary Wick & Buoyant Arrhenius Flame
    {
      id: 'buoyant_arrhenius_combustion',
      stage: 'setup',
      stageNumber: 2,
      title: 'Stefan Phase-Change Melting, Darcy Capillary Transport & Buoyant Arrhenius Kinetics',
      shortName: 'Stefan Phase-Change + Buoyant Arrhenius Reactive Flow',
      active: isPhaseChange || isReactiveFlow,
      triggerSummary:
        isPhaseChange || isReactiveFlow
          ? `Active: Reactive fuel/gas continuum (${Array.from(matIds).filter((m) => ['paraffin_wax', 'cotton_wick', 'air_gas', 'jet_a1_spray'].includes(m)).join(', ') || regime}) in Shape (1) / Setup (2).`
          : 'Standby: Assign a phase-change or reactive-gas continuum material in Shape (1).',
      strongForm:
        'ρ ∂u/∂t + ρ(u·∇)u = −∇p + ∇·(μ ∇u) + (ρ_∞ − ρ) g ẑ,   ρ c_p (∂T/∂t + u·∇T) = ∇·(k ∇T) + ΔH_c ω̇_F + q_solar − 4 σ κ_P (T⁴ − T_∞⁴)',
      weakOrDiscreteForm:
        'ω̇_F = A ρ² Y_F Y_O2 exp(−T_a / T),   u_wick = −(K_w / μ_w)(∇p_c − ρ_w g),   ∂δ_soot/∂t = (ρ_g Y_soot / ρ_soot) max(−v_th · n̂_lens, 0)',
      liveParameters:
        isPhaseChange || isReactiveFlow
          ? [
              isPhaseChange
                ? `Paraffin T_melt = 58 °C (L_f = 210 kJ/kg), Wick T_pyrolysis = 360 °C, Activation temp T_a = 14500 K`
                : `Swirl number S_w = ${readLetParam(doc, 'swirl_number', 0.72).toFixed(2)}, Jet-A1 LHV = 43.2 MJ/kg`,
              `Rosner thermophoretic soot deposition velocity v_th = −0.55 (ν_g / T) ∇T onto overhead optical surface`,
            ]
          : ['Standby operator — assign a phase-change or reactive-flow continuum material to bind live parameters.'],
    },

    // 8. Results Stage: Prager–Synge Dual Equilibrated Upper Compliance Bound & Modal Analysis
    {
      id: 'prager_synge_dual_bounds',
      stage: 'results',
      stageNumber: 3,
      title:
        hasModal && doc.loads.length === 0 && !hasThermalBcs
          ? 'Lanczos Generalized Eigenproblem & Modal Natural Frequencies'
          : 'Prager–Synge Complementary Energy & Certified Compliance Bracketing',
      shortName: 'Certified Dual Compliance Bounds & Modal Eigenvalues',
      active: hasElasticSolve || hasModal,
      triggerSummary: hasElasticSolve
        ? `Active: P2 primal solve + equilibrated dual stress recovery on every solved load case${hasModal ? ` + ${doc.solve.modes} vibration modes` : ''}.`
        : hasModal
          ? `Active: Free-vibration Lanczos modal eigenproblem (${doc.solve.modes} modes) on conforming P2 tetrahedral mesh.`
          : 'Standby: Run a static or modal analysis in Results (3).',
      strongForm:
        hasModal && doc.loads.length === 0 && !hasThermalBcs
          ? '−∇ · σ(φ_m) = ω_m² ρ φ_m  in Ω,   φ_m = 0 on Γ_D,   σ(φ_m) · n = 0 on ∂Ω \\ Γ_D,   f_m = ω_m / (2π)'
          : 'C_h = fᵀ u_h = ∫_Ω ε(u_h) : C : ε(u_h) dΩ  ≤  C_exact  ≤  C_dual(σ*) = ∫_Ω σ* : C⁻¹ : σ* dΩ,   ∇ · σ* + f_b = 0',
      weakOrDiscreteForm:
        hasModal
          ? 'Two-level P1-Cholesky / P2-SGS PCG + Lanczos eigenproblem (K − ω_m² M) φ_m = 0  (f_m = ω_m / 2π)'
          : 'Two-level P1-exact-Cholesky (faer) + P2 block symmetric Gauss–Seidel PCG preconditioner',
      liveParameters: [
        `Target resolution: ${doc.solve.size != null ? `fixed h = ${doc.solve.size} mm` : 'Auto two-rung adaptive ladder (coarse → 150k DOF refined)'}`,
        `Modal eigenvalues requested: ${doc.solve.modes != null ? `${doc.solve.modes} modes` : 'off (static/thermal only)'}`,
      ],
    },

    // 9. Fill Stage: Spatially Graded TPMS Minimal Surfaces, Cubic Orthotropy & Level-Set Evolution
    {
      id: 'tpms_homogenisation_levelset',
      stage: 'fill',
      stageNumber: 4,
      title: 'Multi-Zone TPMS Minimal-Surface Morphing, Cubic Orthotropy & Level-Set Erosion',
      shortName: 'TPMS Lattice Homogenisation & Level-Set Evolution',
      active: hasFill,
      triggerSummary: hasFill
        ? `Active: ${doc.fill!.family}${doc.fill!.morph ? ` → ${doc.fill!.morph}` : ''} (${doc.fill!.mode}, cell ${doc.fill!.cell}${doc.fill!.cell_min ? `→${doc.fill!.cell_min}` : ''} mm, ρ ∈ [${doc.fill!.rho_min}, ${doc.fill!.rho_max}]${hasErode ? `, erode = ${doc.fill!.erode} mm` : ''}) in Fill (4).`
        : 'Standby: Enable a lattice infill in Fill (4) to activate two-pass stress-driven TPMS grading.',
      strongForm:
        'g_gyroid(p) = sin X cos Y + sin Y cos Z + sin Z cos X,   s_blend(p) = (1 − t(p)) d_0(a(p), p) + t(p) d_1(a(p), p)',
      weakOrDiscreteForm:
        'C*_ijkl(ρ, h/a) = C_cubic(E*_11(ρ), E*_12(ρ), G*_44(ρ)),   d_erode(p) = d_body(p) + d_max · (1 − t(p)) · (1 − π_prot(p))',
      liveParameters: hasFill
        ? [
            `Primary family: ${doc.fill!.family} (${doc.fill!.mode}), Morph target: ${doc.fill!.morph ?? 'same'}`,
            `Unit cell wavelength: a_max = ${doc.fill!.cell} mm → a_min = ${doc.fill!.cell_min ?? doc.fill!.cell} mm`,
            `Relative density band: ρ ∈ [${doc.fill!.rho_min}, ${doc.fill!.rho_max}], curve γ = ${doc.fill!.curve}, min_wall = ${doc.fill!.min_wall} mm`,
            `Conformal skin: ${doc.fill!.skin} → ${doc.fill!.skin_max ?? doc.fill!.skin} mm, Outer boundary level-set erode = ${doc.fill!.erode ?? 0} mm`,
          ]
        : ['No lattice fill active (solid continuum solve).'],
    },

    // 10. Make Stage: Anisotropic Tsai–Wu Inter-Layer Peel & Direct Implicit Contour Slicing
    {
      id: 'tsai_wu_peel_implicit_slicing',
      stage: 'make',
      stageNumber: 5,
      title: 'Anisotropic Build-Layer Tsai–Wu Peel Criterion & Direct Implicit Contour Slicing',
      shortName: 'Tsai–Wu Layer Peel & Direct Implicit Slicing',
      active: hasShapes,
      triggerSummary: hasShapes
        ? `Active: Evaluates inter-layer traction t(b̂) = σ · b̂ across build orientation b̂(θ, φ) and slices d_part(x, y, z_k) = 0 directly.`
        : 'Standby: Add geometry in Shape (1).',
      strongForm:
        'σ_n = b̂ · (σ b̂),   τ_s = ‖σ b̂ − σ_n b̂‖,   U_peel(b̂) = √( (max(σ_n, 0) / S_z)² + (τ_s / S_s)² ) ≤ 1',
      weakOrDiscreteForm:
        'Γ_slice(z_k) = { (x, y) ∈ ℝ² : max(d_erode(x, y, z_k), min(d_skin, |s_blend| − w(x, y, z_k)/2)) = 0 }',
      liveParameters: [
        `Build orientation (θ, φ): ${doc.solve.build ? `(${doc.solve.build[0]}°, ${doc.solve.build[1]}°)` : '73-orientation hemisphere sweep (0°..90°)'}`,
        `Inter-layer bond allowables: S_z = ${(docMatSpec.fsmMaterial.yield_z ?? 0.6 * docMatSpec.fsmMaterial.yield_strength).toFixed(0)} MPa, S_s = ${(docMatSpec.fsmMaterial.shear_z ?? docMatSpec.fsmMaterial.yield_strength / Math.sqrt(3)).toFixed(0)} MPa`,
      ],
    },

    // 11. RF VNA Scattering, 50 Ω Smith Chart & Anisotropic CFRP Eddy-Current Coupling
    {
      id: 'vna_smith_cfrp_antenna',
      stage: 'setup',
      stageNumber: 2,
      title: 'Dual-Band RF VNA Scattering (S₁₁, S₂₁), 50 Ω Smith Chart & Anisotropic CFRP Eddy-Current Coupling',
      shortName: 'RF VNA / 50 Ω Smith Chart & CFRP Eddy-Current Lobes',
      active: isDroneAntenna,
      triggerSummary: isDroneAntenna
        ? 'Active: Dual-Band RF Antenna VNA & 3D Lobe parameters in Document.'
        : 'Standby: Add RF antenna parameters or open the RF VNA Workbench.',
      strongForm:
        '∇ × ∇ × E − k₀² ε_r*(r) E = −j ω μ₀ J_feed,   Z_s(CFRP) = (1 + j) / (σ_cfrp δ_skin),   Γ(f) = (Z_in(f) − Z₀) / (Z_in(f) + Z₀)',
      weakOrDiscreteForm:
        'S₁₁(dB) = 20 log₁₀|Γ(f)|,   VSWR = (1 + |Γ|) / (1 − |Γ|),   η_tot = (1 − |Γ|²) · R_rad / (R_rad + R_cfrp(d_cfrp) + R_diel(ε_r, tan δ))',
      liveParameters: isDroneAntenna
        ? [
            `CFRP Standoff d_cfrp = ${readLetParam(doc, 'cfrp_standoff_mm', 24.0).toFixed(1)} mm, Pod Elevation z_pod = ${readLetParam(doc, 'antenna_elev_mm', 16.0).toFixed(1)} mm, Skew α_skew = ${readLetParam(doc, 'gemini_skew_deg', 86.0).toFixed(0)}°`,
            `Dipole Arms: L_900 = ${readLetParam(doc, 'gemini_l900_mm', 136.5).toFixed(1)} mm, L_2400 = ${readLetParam(doc, 'gemini_l2400_mm', 51.2).toFixed(1)} mm, TPU Boot Wall t_tpu = ${readLetParam(doc, 'tpu_pod_wall_mm', 1.8).toFixed(1)} mm`,
            `VNA Match: 915 MHz S₁₁ = ${readLetParam(doc, 'vna_s11_915_db', -23.2).toFixed(1)} dB (η = ${readLetParam(doc, 'vna_eff_915_pct', 80.0).toFixed(1)}%), 2.44 GHz S₁₁ = ${readLetParam(doc, 'vna_s11_2440_db', -28.9).toFixed(1)} dB (η = ${readLetParam(doc, 'vna_eff_2440_pct', 91.6)}%)`,
          ]
        : ['Standby operator — add RF antenna parameters to bind live VNA metrics.'],
    },
  ];
}

export interface VariationalBvpSummary {
  domainLine: string;
  sobolevSpaceLine: string;
  weakStatementLine: string;
  discreteAlgebraicLine: string;
}

/** Synthesise the unified Sobolev function spaces & weak variational problem statement
 * (`Ω, ∂Ω, V_h ⊂ [H¹_D(Ω)]³, a(u_h, v_h) = ℓ(v_h)`) for the active document. */
export function synthesiseVariationalBvpSummary(
  doc: Document,
  solvedSummary?: {
    dofs?: number;
    hMm?: number;
  },
): VariationalBvpSummary {
  const vols = collectVolumeContinuumNodes(doc);
  const solidNodes = vols.filter((v) => v.node.op !== 'subtract');
  const cutNodes = vols.filter((v) => v.node.op === 'subtract');
  const gammaD = doc.supports.map((s) => s.region).join(' ∪ ') || '∅';
  const gammaN = doc.loads.map((l) => l.region).join(' ∪ ') || '∅';
  const gammaThermal = [
    ...doc.temperatures.map((t) => t.region),
    ...doc.convections.map((c) => c.region),
    ...doc.heat_fluxes.map((q) => q.region),
  ].join(' ∪ ');
  const hasThermal = doc.temperatures.length + doc.convections.length + doc.heat_fluxes.length > 0;
  const hasModal = doc.solve.modes != null && doc.solve.modes > 0;
  const hasLoads = doc.loads.length > 0;

  const domainLine =
    vols.length === 0
      ? 'Ω = ∅ ⊂ ℝ³ (add solid primitives in 1. Shape)'
      : `Ω = { x ∈ ℝ³ : d_Ω(x) ≤ 0 } (${solidNodes.length} solid${cutNodes.length > 0 ? `, ${cutNodes.length} boolean cut` : ''}),   ∂Ω = Γ_D (${gammaD}) ∪ Γ_N (${gammaN})${hasThermal ? ` ∪ Γ_th (${gammaThermal})` : ''}`;

  const dofTag =
    solvedSummary?.dofs != null
      ? ` (dim V_h = ${solvedSummary.dofs.toLocaleString()} DOF, h = ${solvedSummary.hMm?.toFixed(2) ?? 'auto'} mm)`
      : ' (10-node quadratic Lagrange P₂ tetrahedra + Keast degree-2 quadrature)';

  const sobolevSpaceLine = hasThermal
    ? `V_h = { v_h ∈ [P₂(𝒯_h)]³ : v_h|_{Γ_D} = 0 } ⊂ [H¹_D(Ω)]³,   W_h = { w_h ∈ P₂(𝒯_h) : w_h|_{Γ_T} = T_D } ⊂ H¹(Ω)${dofTag}`
    : `V_h = { v_h ∈ [P₂(𝒯_h)]³ : v_h|_{Γ_D} = 0 } ⊂ [H¹_D(Ω)]³${dofTag}`;

  const weakStatementLine =
    hasModal && !hasLoads && !hasThermal
      ? `Find eigenpairs (ω_m², φ_m) ∈ ℝ⁺ × V_h (m = 1..${doc.solve.modes}) :   a(φ_m, v_h) = ∫_Ω ε(v_h) : C_e : ε(φ_m) dΩ = ω_m² ∫_Ω ρ φ_m · v_h dΩ   ∀ v_h ∈ V_h`
      : hasThermal
        ? `Find (T_h, u_h) ∈ W_h × V_h :   a_th(T_h, w_h) = ℓ_th(w_h)  ∀ w_h ∈ W_h,0   and   a_el(u_h, v_h) = ∫_Ω ε(v_h) : C_e : ε(u_h) dΩ = ∫_{Γ_N} v_h · t̂ dΓ + ∫_Ω ε(v_h) : C_e : α_e (T_h − T_ref) I dΩ   ∀ v_h ∈ V_h`
        : `Find u_h ∈ V_h :   a(u_h, v_h) = ∫_Ω ε(v_h) : C_e(ρ) : ε(u_h) dΩ = ℓ(v_h) = ∫_{Γ_N} v_h · t̂ dΓ   ∀ v_h ∈ V_h`;

  const discreteAlgebraicLine =
    hasModal && !hasLoads && !hasThermal
      ? `(K − ω_m² M) φ_m = 0,   f_m = ω_m / (2π)   (Shift-invert / two-level P1-Cholesky + P2-SGS Lanczos)`
      : hasThermal
        ? `K_th T = f_th  ⇒  K u = f_mech + f_th(T)   (Two-level P1-Cholesky + P2 block-SGS PCG, Prager–Synge dual bound C_h ≤ C* ≤ C_dual(σ*))`
        : `K u = f   (Two-level P1-Cholesky + P2 block-SGS PCG)${hasModal ? `  +  (K − ω_m² M) φ_m = 0 (${doc.solve.modes} modes)` : ''}  +  Prager–Synge dual bound C_h ≤ C* ≤ C_dual(σ*)`;

  return {
    domainLine,
    sobolevSpaceLine,
    weakStatementLine,
    discreteAlgebraicLine,
  };
}

/** Generate a complete, self-contained Markdown report of the active mathematical formulation
 * at the current design stage. */
export function generateAppliedMathMarkdownReport(
  doc: Document,
  activeLens: string,
  solvedSummary?: {
    dofs?: number;
    hMm?: number;
    maxVmMpa?: number;
    maxDispMm?: number;
    maxTempC?: number;
    massG?: number;
  },
  includeStandby = false,
): string {
  const eqs = evaluateDocumentFieldEquations(doc);
  const shownEqs = includeStandby ? eqs : eqs.filter((e) => e.active);
  const vols = collectVolumeContinuumNodes(doc);
  const regime = detectCoupledPhysicsRegime(doc);
  const activeCount = eqs.filter((e) => e.active).length;
  const bvp = synthesiseVariationalBvpSummary(doc, solvedSummary);

  const lines: string[] = [];
  lines.push(`# Applied Mathematics & Active Field Equations Report`);
  lines.push(`**Document**: \`${doc.name}\`  `);
  lines.push(`**Current Design Stage**: \`${activeLens.toUpperCase()}\`  `);
  lines.push(`**Activated Field Equations**: ${activeCount} / ${eqs.length} governing PDEs active  `);
  if (regime) {
    lines.push(`**Coupled Multiphysics Regime**: \`${regime}\` (auto-activated from volume materials & Setup BCs)  `);
  }
  lines.push('');

  lines.push(`## 0. Synthesised Variational Boundary-Value Problem & Sobolev Spaces`);
  lines.push(`- **Computational Domain & Boundary Partition**: \`${bvp.domainLine}\``);
  lines.push(`- **Trial & Test Function Spaces**: \`${bvp.sobolevSpaceLine}\``);
  lines.push(`- **Weak Variational Formulation**:`);
  lines.push(`  \`\`\`text`);
  lines.push(`  ${bvp.weakStatementLine}`);
  lines.push(`  \`\`\``);
  lines.push(`- **Discrete Algebraic System & Certified Bounds**: \`${bvp.discreteAlgebraicLine}\``);
  lines.push('');

  lines.push(`## 1. Subdomain Continuum & Constitutive Table (\`Shape 1\`)`);
  lines.push(`| Volume Node | CSG Op | Assigned Material | Continuum Phase | Governing Constitutive Law |`);
  lines.push(`|---|---|---|---|---|`);
  for (const v of vols) {
    lines.push(
      `| \`${v.node.id}\` | \`${v.node.op}\` | ${v.spec.name} (\`${v.matId}\`) | **${v.spec.phaseLabel}** | \`${v.spec.constitutiveEquation}\` |`,
    );
  }
  if (vols.length === 0) {
    lines.push(`| *(empty)* | — | \`${doc.material}\` | Solid Elastic | — |`);
  }
  lines.push('');

  lines.push(`## 2. Boundary Conditions & Multiphysics Couplings (\`Setup 2\`)`);
  lines.push(`| Boundary / Parameter | Target Region / Scope | Mathematical Specification |`);
  lines.push(`|---|---|---|`);
  for (const s of doc.supports) {
    const axes = ['x', 'y', 'z'].filter((_, i) => s.fixed[i]).join(', ');
    lines.push(`| Support \`${s.id}\` | \`${s.region}\` (\`Γ_D\`) | Dirichlet displacement \`u_{${axes}} = 0\` |`);
  }
  for (const l of doc.loads) {
    if (l.kind.type === 'force') {
      lines.push(
        `| Force \`${l.id}\` | \`${l.region}\` (\`Γ_N\`) | Neumann traction \`∫_{Γ} σ·n dΓ = (${l.kind.vector.join(', ')}) N\` (Case: ${l.case}) |`,
      );
    } else if (l.kind.type === 'pressure') {
      lines.push(
        `| Pressure \`${l.id}\` | \`${l.region}\` (\`Γ_N\`) | Normal traction \`σ·n = −${l.kind.mpa} MPa · n\` (Case: ${l.case}) |`,
      );
    } else if (l.kind.type === 'preload') {
      lines.push(
        `| Bolt Preload \`${l.id}\` | \`${l.region}\` (\`Γ_N\`) | Axial fastener clamp force \`F_p = ${l.kind.newtons} N\` (Case: ${l.case}) |`,
      );
    } else if (l.kind.type === 'distributed') {
      lines.push(
        `| Spatial Traction \`${l.id}\` | \`${l.region}\` (\`Γ_N\`) | Spatially varying traction \`p(x,y,z) = ${l.kind.expr}\` (Case: ${l.case}) |`,
      );
    } else {
      lines.push(`| Load \`${l.id}\` | \`${l.region}\` (\`Γ_N\`) | \`${(l.kind as { type: string }).type}\` (Case: ${l.case}) |`);
    }
  }
  for (const t of doc.temperatures) {
    lines.push(`| Temperature \`${t.id}\` | \`${t.region}\` (\`Γ_T\`) | Dirichlet temperature \`T = ${t.celsius} °C\` (Case: ${t.case}) |`);
  }
  for (const c of doc.convections) {
    lines.push(
      `| Convection \`${c.id}\` | \`${c.region}\` (\`Γ_R\`) | Robin film \`−k ∇T·n = ${c.h} (T − ${c.t_inf} °C)\` (Case: ${c.case}) |`,
    );
  }
  for (const q of doc.heat_fluxes) {
    lines.push(`| Heat Flux \`${q.id}\` | \`${q.region}\` (\`Γ_q\`) | Neumann heat flux \`−k ∇T·n = ${q.q}\` (Case: ${q.case}) |`);
  }
  for (const l of doc.lets ?? []) {
    lines.push(`| Parameter \`let ${l.name}\` | Global / Coupled PDE | \`${l.name} = ${l.value}\` (expr: \`${l.expr}\`) |`);
  }
  lines.push('');

  lines.push(`## 3. Activated Governing Field Equations Across Design Stages`);
  for (const eq of shownEqs) {
    lines.push(`### [Stage ${eq.stageNumber} · ${eq.stage.toUpperCase()}] ${eq.title} — ${eq.active ? 'ACTIVE' : 'STANDBY'}`);
    lines.push(`- **Activation Status**: ${eq.triggerSummary}`);
    lines.push(`- **Strong Differential Form**:`);
    lines.push(`  \`\`\`text`);
    lines.push(`  ${eq.strongForm}`);
    lines.push(`  \`\`\``);
    lines.push(`- **Weak Variational / Discrete Form**:`);
    lines.push(`  \`\`\`text`);
    lines.push(`  ${eq.weakOrDiscreteForm}`);
    lines.push(`  \`\`\``);
    lines.push(`- **Live Model Parameters**:`);
    for (const p of eq.liveParameters) {
      lines.push(`  - ${p}`);
    }
    lines.push('');
  }

  if (solvedSummary && (solvedSummary.dofs || solvedSummary.maxVmMpa != null)) {
    lines.push(`## 4. Solved Verification & Discretization Readout (\`Results 3\`)`);
    if (solvedSummary.dofs != null) lines.push(`- **Active P2 Degrees of Freedom**: ${solvedSummary.dofs.toLocaleString()} DOF (mesh size h = ${solvedSummary.hMm?.toFixed(2) ?? '—'} mm)`);
    if (solvedSummary.maxVmMpa != null) lines.push(`- **Peak von Mises Stress σ_vM**: ${solvedSummary.maxVmMpa.toFixed(2)} MPa`);
    if (solvedSummary.maxDispMm != null) lines.push(`- **Peak Deflection ‖u‖_∞**: ${solvedSummary.maxDispMm.toFixed(4)} mm`);
    if (solvedSummary.maxTempC != null) lines.push(`- **Peak Nodal Temperature T_max**: ${solvedSummary.maxTempC.toFixed(1)} °C`);
    if (solvedSummary.massG != null) lines.push(`- **Evaluated Component Mass**: ${solvedSummary.massG.toFixed(2)} g`);
    lines.push('');
  }

  return lines.join('\n');
}
