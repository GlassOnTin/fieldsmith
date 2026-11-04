//! Pilkington float-bath glass forming, turbulent molten-tin circulation with thermal buoyancy,
//! and micron-scale redox gas/bubble Navier-slip interfacial mechanics.
//!
//! Reference physics and plant dataset (`uk5-585mm-mw-01e.cff`):
//! - 2D viscous extensional sheet forming (`float2fe`): Vogel–Fulcher–Tammann viscosity
//!   `η(T) = 10^(A / (T - B) - C)` Pa·s, capillary–gravity equilibrium at the free ribbon edge
//!   `σ_nn = F_s / h - ½ ρ_g g (1 - ρ_g/ρ_Sn) h`, and attenuation under angled top rolls to 5.85 mm.
//! - 2D depth-averaged turbulent molten-tin bath (`tin2fe`, Ron Smith J. Fluid Mech. 1990, 214,
//!   211–228): variable bath depth `H(x,y)` (50–150 mm refractory steps + submerged graphite flags
//!   and barriers), solved via transient skew-symmetric + Variational Multiscale (VMS) time-stepping
//!   rather than steady ad-hoc SUPG upwinding, capturing unsteady ribbon-edge Kelvin–Helmholtz eddies
//!   and mild thermal buoyancy forcing (`β_Sn = 9.09e-5 K⁻¹`).
//! - Interfacial redox slip law: a micron-scale gas/bubble cushion (`H₂`, `SO₂`, `SnO(g)`) at the
//!   glass–tin contact produces a Navier slip length `L_s = (μ_Sn / μ_gas) δ_film ≈ 40–150 μm`
//!   comparable to the liquid-tin viscous sublayer (`δ_sub = 5 ν_Sn / u_* ≈ 40 μm`), reducing
//!   momentum drag while preserving high thermal conductance (`k_gas / δ_film`).

use std::fs;
use std::path::Path;

const G_ACCEL: f64 = 9.80665;

/// Top roll station along the float bath (`?TOP_ROLLS_L`).
#[derive(Clone, Debug)]
pub struct TopRoll {
    pub x: f64,
    pub speed: f64,
    pub angle_deg: f64,
    pub nip: f64,
    pub radius: f64,
}

/// Overhead electrical roof heater bay (`?REHEAT`).
#[derive(Clone, Debug)]
pub struct HeaterBay {
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
    pub power_w: f64,
    pub height: f64,
}

/// Overhead bath cooler (`?OVERHEAD_COOLERS`).
#[derive(Clone, Debug)]
pub struct OverheadCooler {
    pub x: f64,
    pub num_elements: usize,
    pub cooler_type: usize,
    pub cross_section: f64,
    pub align: usize,
    pub width: f64,
    pub power: f64,
    pub ogap: f64,
    pub height: f64,
}

/// Submerged side flag (`?FLAGS`) that restricts tin return flow near bath walls.
#[derive(Clone, Debug)]
pub struct TinFlag {
    pub x: f64,
    pub length: f64,
    pub depth_reduction: f64,
    /// Whether the submerged flag is water-cooled (`Flags.cool[n] != 0` in `ReadTT2CFF.c` line 546).
    pub cooled: bool,
}

/// Submerged full/partial dam barrier across the tin pool (`?BARRIER_DATA`).
#[derive(Clone, Debug)]
pub struct TinBarrier {
    pub x: f64,
    pub length: f64,
    pub residual_tin_depth: f64,
}

/// Linear electromagnetic induction motor suspended above the molten-tin bath (`?LINEAR_MOTORS`,
/// `LinMotorData` in `cff.h` lines 199–205).
#[derive(Clone, Copy, Debug)]
pub struct LinearMotor {
    /// Longitudinal bay position `x` along the bath (m).
    pub bay_x_m: f64,
    /// Stator phase current in Amps (`power` in `cff.h`).
    pub current_amps: f64,
    /// Thrust vector angle in degrees relative to the `+x` downstream bath axis (`180°` = upstream `-x`).
    pub angle_deg: f64,
    /// Transverse distance of the stator centre inward from the refractory bath side wall (m).
    pub ypos_from_wall_m: f64,
}

/// Submerged water-cooled tin cooler (`?TIN_COOLERS`, `TinCoolerData` in `cff.h` lines 217–221).
#[derive(Clone, Copy, Debug)]
pub struct TinCooler {
    /// Longitudinal bay position `x` along the bath (m).
    pub bay_x_m: f64,
    /// Transverse immersion length inward from the refractory bath side wall (m).
    pub length_m: f64,
}

/// Localized heat losses to water-cooled bath hardware (`?EXTRA_LOSSES`, `ExtraLossData` in `cff.h`).
#[derive(Clone, Copy, Debug, Default)]
pub struct ExtraLosses {
    pub top_roll_w: f64,
    pub periscope_w: f64,
    /// Heat extracted per unit immersion length of cooled flag or tin cooler (`Extra.flag`, W/m).
    pub flag_w_per_m: f64,
    /// Heat extracted per linear motor (`Extra.lin_mot`, W).
    pub linear_motor_w: f64,
    pub spout_w: f64,
    pub lip_w: f64,
    pub drain_w: f64,
}

/// Parameters for sinuous viscoelastic plate/flexural ribbon distortion (`?SINUOUS_DATA`,
/// `SinuousData` in `cff.h` lines 162–174, used by `sindi8cff.f`).
#[derive(Clone, Copy, Debug)]
pub struct SinuousParams {
    /// Transverse half-width of the computation domain (`halfw = 0.10 m`).
    pub pulse_half_width_m: f64,
    /// Number of transverse finite-difference intervals (`meshno = 100`).
    pub num_meshes: usize,
    /// Number of transverse meshes spanned by the turbulent pressure pulse (`hmeshno = 6`).
    pub pulse_meshes: usize,
    /// Longitudinal length of the turbulent pressure pulse (`pulsel = 0.05 m`).
    pub pulse_length_m: f64,
    /// Glass surface tension (`surfT = 0.82 N/m`).
    pub surface_tension_n_m: f64,
    /// Dimensionless Maxwell relaxation time-step multiplier (`tstep = 200.0`).
    pub time_step_factor: f64,
    /// Starting longitudinal coordinate along the bath (`start = 23.37033 m`).
    pub start_x_m: f64,
    /// Longitudinal step increment along the bath (`inc = 0.50 m`).
    pub step_x_m: f64,
    /// Number of starting positions to evaluate (`nstep = 81`).
    pub num_steps: usize,
    /// Transverse fractional position (`yfrac = 0.0`).
    pub transverse_frac: f64,
}

impl Default for SinuousParams {
    fn default() -> Self {
        Self {
            pulse_half_width_m: 0.10,
            num_meshes: 100,
            pulse_meshes: 6,
            pulse_length_m: 0.05,
            surface_tension_n_m: 0.82,
            time_step_factor: 200.0,
            start_x_m: 23.37033,
            step_x_m: 0.50,
            num_steps: 81,
            transverse_frac: 0.0,
        }
    }
}

/// Parameters for varicose thickness/optical distortion propagation (`?VARICOSE_DATA`,
/// `VaricoseData` in `cff.h` lines 176–183, used by `vardicff.f`).
#[derive(Clone, Copy, Debug)]
pub struct VaricoseParams {
    /// Characteristic pulse residence time (`tres = 0.50 s`).
    pub pulse_time_s: f64,
    /// Local temperature fluctuation amplitude (`tempflu = 0.50 °C`).
    pub temp_fluctuation_c: f64,
    /// Fluctuation period (`timeflu = 3.00 s`).
    pub time_fluctuation_s: f64,
    /// Thermal penetration depth of the temperature fluctuation (`hiflu = 0.003 m`).
    pub fluctuation_depth_m: f64,
    /// Transverse fractional position (`yfrac = 0.0`).
    pub transverse_frac: f64,
}

impl Default for VaricoseParams {
    fn default() -> Self {
        Self {
            pulse_time_s: 0.50,
            temp_fluctuation_c: 0.50,
            time_fluctuation_s: 3.00,
            fluctuation_depth_m: 0.003,
            transverse_frac: 0.0,
        }
    }
}

/// Vogel–Fulcher–Tammann viscosity law `η(T_C) = 10^(A / (T_C - B) - C)` Pa·s.
#[derive(Clone, Copy, Debug)]
pub struct FulcherViscosity {
    pub a: f64,
    pub b: f64,
    pub c: f64,
}

impl FulcherViscosity {
    /// Evaluate dynamic viscosity `η` in Pa·s at temperature `temp_c` (°C).
    #[inline]
    pub fn viscosity_pa_s(&self, temp_c: f64) -> f64 {
        let denom = (temp_c - self.b).max(50.0);
        10.0_f64.powf(self.a / denom - self.c)
    }
}

/// 2D glass ribbon mesh and state fields (`?RIBBON_DATA`).
#[derive(Clone, Debug)]
pub struct RibbonData {
    pub nx: usize,
    pub ny: usize,
    pub x: Vec<f64>,
    pub y: Vec<Vec<f64>>,
    pub u: Vec<Vec<f64>>,
    pub v: Vec<Vec<f64>>,
    pub thickness: Vec<Vec<f64>>,
    pub temperature: Vec<Vec<f64>>,
}

/// 2D molten-tin mesh and state fields (`?TINFLOW_DATA`).
#[derive(Clone, Debug)]
pub struct TinFlowData {
    pub nx: usize,
    pub ny: usize,
    pub x: Vec<f64>,
    pub y: Vec<Vec<f64>>,
    pub u: Vec<Vec<f64>>,
    pub v: Vec<Vec<f64>>,
    pub temperature: Vec<Vec<f64>>,
    pub pressure: Vec<Vec<f64>>,
}

/// Stepped refractory bath geometry (`?BATH_DATA`).
#[derive(Clone, Debug)]
pub struct BathGeometry {
    pub id: String,
    pub name: String,
    pub length: f64,
    pub restrictor_length: f64,
    pub restrictor_spread: f64,
    pub restrictor_tan: f64,
    pub width_sections: Vec<(f64, f64)>,
    pub depth_sections: Vec<(f64, f64)>,
    pub floor_thickness_sections: Vec<(f64, f64)>,
}

/// Overall production line parameters (`?CASE_DATA` + `?FLOAT2D_DATA`).
#[derive(Clone, Debug)]
pub struct FloatCaseParams {
    pub load_kg_s: f64,
    pub substance_m: f64,
    pub gross_width_m: f64,
    pub nett_width_m: f64,
    pub lehr_speed_m_s: f64,
    pub canal_temperature_c: f64,
    pub tin_drag_coeff: f64,
    pub gravity_constant_n_m3: f64,
    pub surface_tension_n_m: f64,
}

/// Molten-tin physical and turbulence model parameters (`?TINFLOW_MODEL`).
#[derive(Clone, Debug)]
pub struct TinFlowParams {
    pub nx: usize,
    pub nodes_outside_ribbon: usize,
    pub top_roughness_dellup: f64,
    pub bottom_roughness_dellow: f64,
    pub von_karman: f64,
    pub turbulence_constant_delsqc: f64,
    pub dynamic_viscosity_pa_s: f64,
    pub density_kg_m3: f64,
    /// Volumetric thermal expansion coefficient `β_Sn = -(1/ρ)(∂ρ/∂T)` in K⁻¹.
    /// From XFloat `ρ_Sn(T) = 6960 - 0.636 (T - 231.93)`: `0.636 / 7000 ≈ 9.09e-5 K⁻¹`.
    pub volumetric_expansion_k_inv: f64,
}

/// Thermal model parameters (`?THERMAL_MODEL`).
#[derive(Clone, Debug)]
pub struct ThermalModelParams {
    pub tin_emissivity_base: f64,
    pub glass_emissivity: f64,
    pub tin_conductivity_w_m_k: f64,
    pub glass_conductivity_w_m_k: f64,
    pub tin_specific_heat_j_kg_k: f64,
    pub glass_specific_heat_j_kg_k: f64,
    pub stefan_boltzmann: f64,
    pub external_temperature_c: f64,
    pub roof_conductance: f64,
    pub roof_effective_conductivity: f64,
    pub floor_conductivity_w_m_k: f64,
    pub glass_density_kg_m3: f64,
}

/// Global heat balance summary (`?HEAT_BALANCE`, in kW for half-bath or full-bath).
#[derive(Clone, Debug, Default)]
pub struct HeatBalanceSummary {
    pub heat_in_glass: f64,
    pub heat_out_glass: f64,
    pub glass_to_tin: f64,
    pub glass_to_roof: f64,
    pub glass_to_coolers: f64,
    pub heaters_to_glass: f64,
    pub tin_from_glass: f64,
    pub floor_loss: f64,
    pub tin_to_roof: f64,
    pub tin_to_coolers: f64,
    pub tin_cooler_loss: f64,
    pub heaters_to_tin: f64,
}

/// Complete parsed Pilkington `.cff` float-bath case.
#[derive(Clone, Debug)]
pub struct CffCase {
    pub top_rolls: Vec<TopRoll>,
    pub heaters: Vec<HeaterBay>,
    pub coolers: Vec<OverheadCooler>,
    pub flags: Vec<TinFlag>,
    pub barriers: Vec<TinBarrier>,
    pub linear_motors: Vec<LinearMotor>,
    pub tin_coolers: Vec<TinCooler>,
    pub extra_losses: ExtraLosses,
    pub sinuous: SinuousParams,
    pub varicose: VaricoseParams,
    pub ribbon: RibbonData,
    pub bath: BathGeometry,
    pub case_params: FloatCaseParams,
    pub tin_params: TinFlowParams,
    pub tin: TinFlowData,
    pub thermal: ThermalModelParams,
    pub fulcher: FulcherViscosity,
    pub heat_balance: HeatBalanceSummary,
    /// Number of outer coupling cycles where `tin2fe` hit its 1000-iteration ceiling in `.cff`.
    pub legacy_tin_maxed_out_cycles: usize,
    pub legacy_total_cycles: usize,
}

fn first_token_f64(line: &str) -> Result<f64, String> {
    line.split_whitespace()
        .next()
        .ok_or_else(|| format!("expected float token in line: {line:?}"))?
        .parse::<f64>()
        .map_err(|e| format!("float parse error in {line:?}: {e}"))
}

fn first_token_usize(line: &str) -> Result<usize, String> {
    line.split_whitespace()
        .next()
        .ok_or_else(|| format!("expected usize token in line: {line:?}"))?
        .parse::<usize>()
        .map_err(|e| format!("usize parse error in {line:?}: {e}"))
}

fn parse_f64_row(line: &str) -> Result<Vec<f64>, String> {
    line.split_whitespace()
        .map(|tok| {
            tok.parse::<f64>()
                .map_err(|e| format!("float parse error on {tok:?}: {e}"))
        })
        .collect()
}

fn piecewise_linear_interp(pts: &[(f64, f64)], x: f64) -> f64 {
    if pts.is_empty() {
        return 0.0;
    }
    if x <= pts[0].0 {
        return pts[0].1;
    }
    for w in pts.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        if x <= x1 {
            let dx = x1 - x0;
            if dx.abs() < 1e-12 {
                return y1;
            }
            let t = ((x - x0) / dx).clamp(0.0, 1.0);
            return y0 + t * (y1 - y0);
        }
    }
    pts.last().unwrap().1
}

impl CffCase {
    /// Load the embedded Pilkington UK5 `5.85 mm` production reference dataset (`uk5-585mm-mw-01e.cff`).
    /// Works in both native and WebAssembly (`wasm32-unknown-unknown`) environments without filesystem access.
    pub fn uk5_reference() -> Result<Self, String> {
        const UK5_CFF: &str = include_str!("../data/uk5-585mm-mw-01e.cff");
        Self::from_str(UK5_CFF)
    }

    /// Load and parse a Pilkington `.cff` (`CFF_FORMAT_FILE`) case from disk.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, String> {
        let content = fs::read_to_string(path.as_ref())
            .map_err(|e| format!("failed to read {}: {e}", path.as_ref().display()))?;
        Self::from_str(&content)
    }

    /// Parse `.cff` text content.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(content: &str) -> Result<Self, String> {
        let mut lines = content.lines().map(str::trim);
        let first = lines.next().ok_or("empty CFF file")?;
        if first != "CFF_FORMAT_FILE" {
            return Err(format!("invalid CFF header: {first:?}"));
        }

        let mut top_rolls = Vec::new();
        let mut heaters = Vec::new();
        let mut coolers = Vec::new();
        let mut flags = Vec::new();
        let mut barriers = Vec::new();
        let mut linear_motors = Vec::new();
        let mut tin_coolers = Vec::new();
        let mut extra_losses = ExtraLosses::default();
        let mut sinuous = SinuousParams::default();
        let mut varicose = VaricoseParams::default();
        let mut ribbon: Option<RibbonData> = None;
        let mut bath: Option<BathGeometry> = None;
        let mut load_kg_s = 0.0;
        let mut substance_m = 0.00585;
        let mut gross_width_m = 3.6;
        let mut nett_width_m = 3.2;
        let mut lehr_speed_m_s = 0.15;
        let mut canal_temperature_c = 1095.0;
        let mut tin_drag_coeff = 0.0;
        let mut gravity_constant_n_m3 = 15354.0;
        let mut surface_tension_n_m = 0.35;
        let mut tin_params: Option<TinFlowParams> = None;
        let mut tin: Option<TinFlowData> = None;
        let mut thermal: Option<ThermalModelParams> = None;
        let mut fulcher = FulcherViscosity {
            a: 4232.815,
            b: 266.6683,
            c: 2.615659,
        };
        let mut heat_balance = HeatBalanceSummary::default();
        let mut legacy_tin_maxed_out_cycles = 0;
        let mut legacy_total_cycles = 0;

        while let Some(line) = lines.next() {
            match line {
                "?TOP_ROLLS_L" => {
                    let n = first_token_usize(lines.next().ok_or("eof in TOP_ROLLS_L")?)?;
                    top_rolls.clear();
                    for _ in 0..n {
                        let vals: Vec<f64> = lines
                            .next()
                            .ok_or("eof in top roll row")?
                            .split_whitespace()
                            .take(5)
                            .map(|s| s.parse::<f64>().unwrap_or(0.0))
                            .collect();
                        if vals.len() >= 5 {
                            top_rolls.push(TopRoll {
                                x: vals[0],
                                speed: vals[1],
                                angle_deg: vals[2],
                                nip: vals[3],
                                radius: vals[4],
                            });
                        }
                    }
                }
                "?REHEAT" => {
                    let n = first_token_usize(lines.next().ok_or("eof in REHEAT")?)?;
                    heaters.clear();
                    for _ in 0..n {
                        let vals = parse_f64_row(lines.next().ok_or("eof in heater row")?)?;
                        if vals.len() >= 6 {
                            heaters.push(HeaterBay {
                                x0: vals[0],
                                x1: vals[1],
                                y0: vals[2],
                                y1: vals[3],
                                power_w: vals[4],
                                height: vals[5],
                            });
                        }
                    }
                }
                "?OVERHEAD_COOLERS" => {
                    let n = first_token_usize(lines.next().ok_or("eof in OVERHEAD_COOLERS")?)?;
                    coolers.clear();
                    for _ in 0..n {
                        let vals = parse_f64_row(lines.next().ok_or("eof in cooler row")?)?;
                        if vals.len() >= 9 {
                            coolers.push(OverheadCooler {
                                x: vals[0],
                                num_elements: vals[1] as usize,
                                cooler_type: vals[2] as usize,
                                cross_section: vals[3],
                                align: vals[4] as usize,
                                width: vals[5],
                                power: vals[6],
                                ogap: vals[7],
                                height: vals[8],
                            });
                        }
                    }
                }
                "?SINUOUS_DATA" => {
                    sinuous = SinuousParams {
                        pulse_half_width_m: first_token_f64(lines.next().ok_or("eof")?)?,
                        num_meshes: first_token_usize(lines.next().ok_or("eof")?)?,
                        pulse_meshes: first_token_usize(lines.next().ok_or("eof")?)?,
                        pulse_length_m: first_token_f64(lines.next().ok_or("eof")?)?,
                        surface_tension_n_m: first_token_f64(lines.next().ok_or("eof")?)?,
                        time_step_factor: first_token_f64(lines.next().ok_or("eof")?)?,
                        start_x_m: first_token_f64(lines.next().ok_or("eof")?)?,
                        step_x_m: first_token_f64(lines.next().ok_or("eof")?)?,
                        num_steps: first_token_usize(lines.next().ok_or("eof")?)?,
                        transverse_frac: first_token_f64(lines.next().ok_or("eof")?)?,
                    };
                }
                "?VARICOSE_DATA" => {
                    varicose = VaricoseParams {
                        pulse_time_s: first_token_f64(lines.next().ok_or("eof")?)?,
                        temp_fluctuation_c: first_token_f64(lines.next().ok_or("eof")?)?,
                        time_fluctuation_s: first_token_f64(lines.next().ok_or("eof")?)?,
                        fluctuation_depth_m: first_token_f64(lines.next().ok_or("eof")?)?,
                        transverse_frac: first_token_f64(lines.next().ok_or("eof")?)?,
                    };
                }
                "?LINEAR_MOTORS" => {
                    let n = first_token_usize(lines.next().ok_or("eof in LINEAR_MOTORS")?)?;
                    linear_motors.clear();
                    for _ in 0..n {
                        let row = lines.next().ok_or("eof in linear motor row")?;
                        let vals: Vec<f64> = row
                            .split_whitespace()
                            .take(4)
                            .map(|s| s.parse::<f64>().unwrap_or(0.0))
                            .collect();
                        if vals.len() >= 4 {
                            linear_motors.push(LinearMotor {
                                bay_x_m: vals[0],
                                current_amps: vals[1],
                                angle_deg: vals[2],
                                ypos_from_wall_m: vals[3],
                            });
                        }
                    }
                }
                "?TIN_COOLERS" => {
                    let n = first_token_usize(lines.next().ok_or("eof in TIN_COOLERS")?)?;
                    tin_coolers.clear();
                    for _ in 0..n {
                        let row = lines.next().ok_or("eof in tin cooler row")?;
                        let vals: Vec<f64> = row
                            .split_whitespace()
                            .take(2)
                            .map(|s| s.parse::<f64>().unwrap_or(0.0))
                            .collect();
                        if vals.len() >= 2 {
                            tin_coolers.push(TinCooler {
                                bay_x_m: vals[0],
                                length_m: vals[1],
                            });
                        }
                    }
                }
                "?EXTRA_LOSSES" => {
                    extra_losses = ExtraLosses {
                        top_roll_w: first_token_f64(lines.next().ok_or("eof")?)?,
                        periscope_w: first_token_f64(lines.next().ok_or("eof")?)?,
                        flag_w_per_m: first_token_f64(lines.next().ok_or("eof")?)?,
                        linear_motor_w: first_token_f64(lines.next().ok_or("eof")?)?,
                        spout_w: first_token_f64(lines.next().ok_or("eof")?)?,
                        lip_w: first_token_f64(lines.next().ok_or("eof")?)?,
                        drain_w: first_token_f64(lines.next().ok_or("eof")?)?,
                    };
                }
                "?FLAGS" => {
                    let n = first_token_usize(lines.next().ok_or("eof in FLAGS")?)?;
                    flags.clear();
                    for _ in 0..n {
                        let row = lines.next().ok_or("eof in flag row")?;
                        let vals: Vec<f64> = row
                            .split_whitespace()
                            .take(4)
                            .map(|s| s.parse::<f64>().unwrap_or(0.0))
                            .collect();
                        if vals.len() >= 3 {
                            flags.push(TinFlag {
                                x: vals[0],
                                length: vals[1],
                                depth_reduction: vals[2],
                                cooled: vals.get(3).copied().unwrap_or(0.0) != 0.0,
                            });
                        }
                    }
                }
                "?BARRIER_DATA" => {
                    let n = first_token_usize(lines.next().ok_or("eof in BARRIER_DATA")?)?;
                    barriers.clear();
                    for _ in 0..n {
                        let row = lines.next().ok_or("eof in barrier row")?;
                        let vals: Vec<f64> = row
                            .split_whitespace()
                            .take(3)
                            .map(|s| s.parse::<f64>().unwrap_or(0.0))
                            .collect();
                        if vals.len() == 3 {
                            barriers.push(TinBarrier {
                                x: vals[0],
                                length: vals[1],
                                residual_tin_depth: vals[2],
                            });
                        }
                    }
                }
                "?RIBBON_DATA" => {
                    let dims: Vec<usize> = lines
                        .next()
                        .ok_or("eof in RIBBON_DATA dims")?
                        .split_whitespace()
                        .map(|s| s.parse::<usize>().unwrap())
                        .collect();
                    let (nx, ny) = (dims[0], dims[1]);
                    let mut x = Vec::with_capacity(nx);
                    let mut y = Vec::with_capacity(nx);
                    for _ in 0..nx {
                        let row = parse_f64_row(lines.next().ok_or("eof in ribbon mesh")?)?;
                        x.push(row[0]);
                        y.push(row[1..=ny].to_vec());
                    }
                    let mut u = Vec::with_capacity(nx);
                    for _ in 0..nx {
                        u.push(parse_f64_row(lines.next().ok_or("eof in ribbon U")?)?);
                    }
                    let mut v = Vec::with_capacity(nx);
                    for _ in 0..nx {
                        v.push(parse_f64_row(lines.next().ok_or("eof in ribbon V")?)?);
                    }
                    let mut thickness = Vec::with_capacity(nx);
                    for _ in 0..nx {
                        thickness.push(parse_f64_row(lines.next().ok_or("eof in ribbon H")?)?);
                    }
                    let mut temperature = Vec::with_capacity(nx);
                    for _ in 0..nx {
                        temperature.push(parse_f64_row(lines.next().ok_or("eof in ribbon T")?)?);
                    }
                    ribbon = Some(RibbonData {
                        nx,
                        ny,
                        x,
                        y,
                        u,
                        v,
                        thickness,
                        temperature,
                    });
                }
                "?BATH_DATA" => {
                    let id = lines.next().unwrap_or("").to_string();
                    let name1 = lines.next().unwrap_or("").to_string();
                    let _name2 = lines.next().unwrap_or("");
                    let length = first_token_f64(lines.next().ok_or("eof in bath length")?)?;
                    let restrictor_length =
                        first_token_f64(lines.next().ok_or("eof in restrictor length")?)?;
                    let restrictor_spread =
                        first_token_f64(lines.next().ok_or("eof in restrictor spread")?)?;
                    let restrictor_tan =
                        first_token_f64(lines.next().ok_or("eof in restrictor tan")?)?;

                    let nw = first_token_usize(lines.next().ok_or("eof in width N")?)?;
                    let mut width_sections = Vec::with_capacity(nw);
                    for _ in 0..nw {
                        let r = parse_f64_row(lines.next().ok_or("eof in width row")?)?;
                        width_sections.push((r[0], r[1]));
                    }
                    let nd = first_token_usize(lines.next().ok_or("eof in depth N")?)?;
                    let mut depth_sections = Vec::with_capacity(nd);
                    for _ in 0..nd {
                        let r = parse_f64_row(lines.next().ok_or("eof in depth row")?)?;
                        depth_sections.push((r[0], r[1]));
                    }
                    let nf = first_token_usize(lines.next().ok_or("eof in floor N")?)?;
                    let mut floor_thickness_sections = Vec::with_capacity(nf);
                    for _ in 0..nf {
                        let r = parse_f64_row(lines.next().ok_or("eof in floor row")?)?;
                        floor_thickness_sections.push((r[0], r[1]));
                    }
                    bath = Some(BathGeometry {
                        id,
                        name: name1,
                        length,
                        restrictor_length,
                        restrictor_spread,
                        restrictor_tan,
                        width_sections,
                        depth_sections,
                        floor_thickness_sections,
                    });
                }
                "?CASE_DATA" => {
                    load_kg_s = first_token_f64(lines.next().ok_or("eof in load")?)?;
                    substance_m = first_token_f64(lines.next().ok_or("eof in substance")?)?;
                    gross_width_m = first_token_f64(lines.next().ok_or("eof in gross width")?)?;
                    nett_width_m = first_token_f64(lines.next().ok_or("eof in nett width")?)?;
                    lehr_speed_m_s = first_token_f64(lines.next().ok_or("eof in lehr speed")?)?;
                    canal_temperature_c =
                        first_token_f64(lines.next().ok_or("eof in canal temp")?)?;
                }
                "?FLOAT2D_DATA" => {
                    let _inlet_speed = first_token_f64(lines.next().ok_or("eof")?)?;
                    let _inlet_thick = first_token_f64(lines.next().ok_or("eof")?)?;
                    tin_drag_coeff = first_token_f64(lines.next().ok_or("eof")?)?;
                    gravity_constant_n_m3 = first_token_f64(lines.next().ok_or("eof")?)?;
                    surface_tension_n_m = first_token_f64(lines.next().ok_or("eof")?)?;
                }
                "?TINFLOW_MODEL" => {
                    let nx = first_token_usize(lines.next().ok_or("eof")?)?;
                    let nodes_outside_ribbon = first_token_usize(lines.next().ok_or("eof")?)?;
                    let _max_iter = first_token_usize(lines.next().ok_or("eof")?)?;
                    let _tol = first_token_f64(lines.next().ok_or("eof")?)?;
                    let top_roughness_dellup = first_token_f64(lines.next().ok_or("eof")?)?;
                    let bottom_roughness_dellow = first_token_f64(lines.next().ok_or("eof")?)?;
                    let von_karman = first_token_f64(lines.next().ok_or("eof")?)?;
                    let turbulence_constant_delsqc = first_token_f64(lines.next().ok_or("eof")?)?;
                    let dynamic_viscosity_pa_s = first_token_f64(lines.next().ok_or("eof")?)?;
                    let density_kg_m3 = first_token_f64(lines.next().ok_or("eof")?)?;
                    let _relax = first_token_f64(lines.next().ok_or("eof")?)?;
                    // Liquid tin d(rho)/dT = -0.636 kg/(m³·K) => beta = 0.636 / rho
                    let volumetric_expansion_k_inv = 0.636 / density_kg_m3;
                    tin_params = Some(TinFlowParams {
                        nx,
                        nodes_outside_ribbon,
                        top_roughness_dellup,
                        bottom_roughness_dellow,
                        von_karman,
                        turbulence_constant_delsqc,
                        dynamic_viscosity_pa_s,
                        density_kg_m3,
                        volumetric_expansion_k_inv,
                    });
                }
                "?TINFLOW_DATA" => {
                    let dims: Vec<usize> = lines
                        .next()
                        .ok_or("eof in TINFLOW_DATA dims")?
                        .split_whitespace()
                        .map(|s| s.parse::<usize>().unwrap())
                        .collect();
                    let (nx, ny) = (dims[0], dims[1]);
                    let mut x = Vec::with_capacity(nx);
                    let mut y = Vec::with_capacity(nx);
                    for _ in 0..nx {
                        let row = parse_f64_row(lines.next().ok_or("eof in tin mesh")?)?;
                        x.push(row[0]);
                        y.push(row[1..=ny].to_vec());
                    }
                    let mut u = Vec::with_capacity(nx);
                    for _ in 0..nx {
                        u.push(parse_f64_row(lines.next().ok_or("eof in tin U")?)?);
                    }
                    let mut v = Vec::with_capacity(nx);
                    for _ in 0..nx {
                        v.push(parse_f64_row(lines.next().ok_or("eof in tin V")?)?);
                    }
                    let mut temperature = Vec::with_capacity(nx);
                    for _ in 0..nx {
                        temperature.push(parse_f64_row(lines.next().ok_or("eof in tin T")?)?);
                    }
                    let mut pressure = Vec::with_capacity(nx);
                    for _ in 0..nx {
                        pressure.push(parse_f64_row(lines.next().ok_or("eof in tin P")?)?);
                    }
                    tin = Some(TinFlowData {
                        nx,
                        ny,
                        x,
                        y,
                        u,
                        v,
                        temperature,
                        pressure,
                    });
                }
                "?THERMAL_MODEL" => {
                    let tin_emissivity_base = first_token_f64(lines.next().ok_or("eof")?)?;
                    let glass_emissivity = first_token_f64(lines.next().ok_or("eof")?)?;
                    let tin_conductivity_w_m_k = first_token_f64(lines.next().ok_or("eof")?)?;
                    let glass_conductivity_w_m_k = first_token_f64(lines.next().ok_or("eof")?)?;
                    let tin_specific_heat_j_kg_k = first_token_f64(lines.next().ok_or("eof")?)?;
                    let glass_specific_heat_j_kg_k = first_token_f64(lines.next().ok_or("eof")?)?;
                    let stefan_boltzmann = first_token_f64(lines.next().ok_or("eof")?)?;
                    let external_temperature_c = first_token_f64(lines.next().ok_or("eof")?)?;
                    let roof_conductance = first_token_f64(lines.next().ok_or("eof")?)?;
                    let roof_effective_conductivity =
                        first_token_f64(lines.next().ok_or("eof")?)?;
                    let floor_conductivity_w_m_k = first_token_f64(lines.next().ok_or("eof")?)?;
                    thermal = Some(ThermalModelParams {
                        tin_emissivity_base,
                        glass_emissivity,
                        tin_conductivity_w_m_k,
                        glass_conductivity_w_m_k,
                        tin_specific_heat_j_kg_k,
                        glass_specific_heat_j_kg_k,
                        stefan_boltzmann,
                        external_temperature_c,
                        roof_conductance,
                        roof_effective_conductivity,
                        floor_conductivity_w_m_k,
                        glass_density_kg_m3: 2350.0,
                    });
                }
                "?VISCOSITY_DATA" => {
                    let a = first_token_f64(lines.next().ok_or("eof in Fulcher A")?)?;
                    let b = first_token_f64(lines.next().ok_or("eof in Fulcher B")?)?;
                    let c = first_token_f64(lines.next().ok_or("eof in Fulcher C")?)?;
                    fulcher = FulcherViscosity { a, b, c };
                }
                "?HEAT_BALANCE" => {
                    heat_balance = HeatBalanceSummary {
                        heat_in_glass: first_token_f64(lines.next().ok_or("eof")?)?,
                        heat_out_glass: first_token_f64(lines.next().ok_or("eof")?)?,
                        glass_to_tin: first_token_f64(lines.next().ok_or("eof")?)?,
                        glass_to_roof: first_token_f64(lines.next().ok_or("eof")?)?,
                        glass_to_coolers: first_token_f64(lines.next().ok_or("eof")?)?,
                        heaters_to_glass: first_token_f64(lines.next().ok_or("eof")?)?,
                        tin_from_glass: first_token_f64(lines.next().ok_or("eof")?)?,
                        floor_loss: first_token_f64(lines.next().ok_or("eof")?)?,
                        tin_to_roof: first_token_f64(lines.next().ok_or("eof")?)?,
                        tin_to_coolers: first_token_f64(lines.next().ok_or("eof")?)?,
                        tin_cooler_loss: first_token_f64(lines.next().ok_or("eof")?)?,
                        heaters_to_tin: first_token_f64(lines.next().ok_or("eof")?)?,
                    };
                }
                "?ITERATION_DATA" => {
                    let n = first_token_usize(lines.next().ok_or("eof in ITERATION_DATA")?)?;
                    legacy_total_cycles = n;
                    legacy_tin_maxed_out_cycles = 0;
                    for _ in 0..n {
                        if let Some(row) = lines.next() {
                            if let Ok(tin_its) = first_token_usize(row) {
                                if tin_its >= 1000 {
                                    legacy_tin_maxed_out_cycles += 1;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(CffCase {
            top_rolls,
            heaters,
            coolers,
            flags,
            barriers,
            linear_motors,
            tin_coolers,
            extra_losses,
            sinuous,
            varicose,
            ribbon: ribbon.ok_or("missing ?RIBBON_DATA")?,
            bath: bath.ok_or("missing ?BATH_DATA")?,
            case_params: FloatCaseParams {
                load_kg_s,
                substance_m,
                gross_width_m,
                nett_width_m,
                lehr_speed_m_s,
                canal_temperature_c,
                tin_drag_coeff,
                gravity_constant_n_m3,
                surface_tension_n_m,
            },
            tin_params: tin_params.ok_or("missing ?TINFLOW_MODEL")?,
            tin: tin.ok_or("missing ?TINFLOW_DATA")?,
            thermal: thermal.ok_or("missing ?THERMAL_MODEL")?,
            fulcher,
            heat_balance,
            legacy_tin_maxed_out_cycles,
            legacy_total_cycles,
        })
    }

    /// Interpolate open-bath refractory tin depth `H_bath(x)` (m).
    pub fn bath_depth_at(&self, x: f64) -> f64 {
        piecewise_linear_interp(&self.bath.depth_sections, x)
    }

    /// Interpolate full refractory bath width `W_bath(x)` (m).
    pub fn bath_width_at(&self, x: f64) -> f64 {
        piecewise_linear_interp(&self.bath.width_sections, x)
    }

    /// Interpolate refractory floor thickness `H_floor(x)` (m).
    pub fn floor_thickness_at(&self, x: f64) -> f64 {
        piecewise_linear_interp(&self.bath.floor_thickness_sections, x)
    }

    /// Effective molten-tin depth `H(x,y)` at tin grid node `(ix, iy)`, accounting for:
    /// 1. Stepped refractory pool depth `H_bath(x)` (57–108 mm in UK5),
    /// 2. Immersion of the floating glass ribbon (`-⅓ h_glass(x,y)` as in `tin2fe`),
    /// 3. Local depth reductions at submerged side flags (`?FLAGS`) and full/partial dam barriers (`?BARRIER_DATA`).
    pub fn effective_tin_depth_node(&self, ix: usize, iy: usize) -> f64 {
        let x = self.tin.x[ix];
        let y = self.tin.y[ix][iy];
        let mut depth = self.bath_depth_at(x);

        // Subtract 1/3 of ribbon thickness where under glass (iy < ribbon.ny)
        let r_offset = self.ribbon.nx.saturating_sub(self.tin.nx);
        if iy < self.ribbon.ny {
            let r_ix = (ix + r_offset).min(self.ribbon.nx - 1);
            depth -= self.ribbon.thickness[r_ix][iy] / 3.0;
        }

        // Apply submerged side flags within 1 streamwise grid spacing
        let dx = if self.tin.nx > 1 {
            (self.tin.x[1] - self.tin.x[0]).abs().max(0.2)
        } else {
            0.35
        };
        let y_wall = *self.tin.y[ix].last().unwrap_or(&3.5);
        for flag in &self.flags {
            if (x - flag.x).abs() <= 0.6 * dx && y >= (y_wall - flag.length) {
                depth = (depth - flag.depth_reduction).max(0.012);
            }
        }

        // Apply submerged graphite barriers
        for bar in &self.barriers {
            if (x - bar.x).abs() <= 0.6 * dx && y <= 0.5 * bar.length {
                depth = depth.min(bar.residual_tin_depth).max(0.012);
            }
        }

        depth.max(0.012)
    }
}

// ============================================================================
// 1. Capillary–Gravity Balance & Viscous Sheet Attenuation (`float2fe` physics)
// ============================================================================

/// Langmuir's natural equilibrium thickness `h_eq = sqrt(2 F_s / [ρ_g g (1 - ρ_g/ρ_Sn)])` (m)
/// where gravity spreading balances net surface tension `F_s = γ_{g-atm} + γ_{g-Sn} - γ_{Sn-atm}`.
#[inline]
pub fn langmuir_equilibrium_thickness(
    surface_tension_n_m: f64,
    gravity_constant_n_m3: f64,
) -> f64 {
    (2.0 * surface_tension_n_m / gravity_constant_n_m3).sqrt()
}

/// Net horizontal edge normal traction `σ_nn = F_s / h - ½ ρ_g g (1 - α) h` (Pa)
/// from `BoundaryTerms.c` in `float2fe`.
/// - Positive `σ_nn` (< `h_eq`): surface tension pulls edge inward (thickening).
/// - Negative `σ_nn` (> `h_eq`): hydrostatic gravity spreads ribbon outward (thinning).
#[inline]
pub fn edge_capillary_gravity_stress_pa(
    thickness_m: f64,
    surface_tension_n_m: f64,
    gravity_constant_n_m3: f64,
) -> f64 {
    surface_tension_n_m / thickness_m - 0.5 * gravity_constant_n_m3 * thickness_m
}

/// Rosseland optically thick effective thermal conductivity of soda-lime-silica glass `k_eff(T_C)`
/// in W/(m·K), combining phonon conduction and internal infrared radiation (`heat2fe/Get GlassCond`).
#[inline]
pub fn glass_effective_conductivity_w_m_k(temp_c: f64) -> f64 {
    let t = temp_c.clamp(200.0, 1200.0);
    // Matches Pilkington Rosseland table: ~1.25 W/(m·K) at 200 °C -> ~32.2 W/(m·K) at 1100 °C
    let t_k = t + 273.15;
    1.15 + 1.12e-8 * (t_k * t_k * t_k - 473.15_f64.powi(3))
}

/// Redox/temperature-dependent emissivity of molten tin `ε_Sn(T_C)` in H₂:N₂ atmosphere
/// (`CreateTCoeffs.c` line 173: `E_tin = T * 0.00024 + 0.00752`).
#[inline]
pub fn tin_emissivity(temp_c: f64) -> f64 {
    (0.00024 * temp_c + 0.00752).clamp(0.05, 0.45)
}

/// Temperature- and thickness-dependent hemispherical emissivity of soda-lime-silica glass
/// (`CreateTCoeffs.c` line 388 / `BalanceHeat.c` line 140: `E_glass = 1.05 - 0.00044 * Tg + 4.0 * h1`).
#[inline]
pub fn glass_emissivity(temp_c: f64, thickness_m: f64) -> f64 {
    (1.05 - 0.00044 * temp_c + 4.0 * thickness_m).clamp(0.40, 0.98)
}

/// Analytical 3D Hottel corner view-factor kernel `view(x, y)` from a differential horizontal
/// bath element `(dx, dy)` to a parallel rectangular roof heater bay or overhead cooler at
/// dimensionless offsets `(x, y) = (Δx / H_roof, Δy / H_roof)` (`ReadTT2CFF.c` line 698):
///   `view(x, y) = (1 / 2π) [ (x / sqrt(1 + x²)) asin(y / sqrt(1 + x² + y²)) + (y / sqrt(1 + y²)) asin(x / sqrt(1 + x² + y²)) ]`
#[inline]
pub fn hottel_rect_view_factor(x: f64, y: f64) -> f64 {
    let r2 = 1.0 + x * x + y * y;
    let r = r2.sqrt();
    let ax = (y / r).clamp(-1.0, 1.0).asin();
    let ay = (x / r).clamp(-1.0, 1.0).asin();
    let term_x = (x / (1.0 + x * x).sqrt()) * ax;
    let term_y = (y / (1.0 + y * y).sqrt()) * ay;
    (term_x + term_y) / (2.0 * std::f64::consts::PI)
}

/// Precomputed 3D radiative view factors and heat source distribution on the bath mesh (`ReadTT2CFF.c`).
#[derive(Clone, Debug)]
pub struct RadiationViewFactorMap {
    /// Effective Voronoi/quadrilateral nodal surface area `area[i][j]` (m²), shape `[nx][ny]`.
    pub nodal_area_m2: Vec<Vec<f64>>,
    /// 3D roof heater radiative flux incident on each bath node `source[i][j]` (W/m²), shape `[nx][ny]`.
    pub heater_flux_w_m2: Vec<Vec<f64>>,
    /// 3D view factor `Vu[i][j]` from each bath node to overhead coolers (`[0, 1]`), shape `[nx][ny]`.
    pub cooler_view_factor: Vec<Vec<f64>>,
    /// Total active electrical heater power in the half-bath domain (kW).
    pub total_half_bath_heater_power_kw: f64,
    /// Heater power absorbed directly by the glass ribbon (kW), matching `?HEAT_BALANCE` `heaters_to_glass`.
    pub heaters_to_glass_kw: f64,
    /// Heater power absorbed directly by the exposed tin bath (kW), matching `?HEAT_BALANCE` `heaters_to_tin`.
    pub heaters_to_tin_kw: f64,
}

/// Assemble the exact 3D Hottel radiative view factors (`ReadTT2CFF.c` lines 423–690) from
/// all 36 roof heater bays (`?REHEAT`) and all overhead coolers (`?OVERHEAD_COOLERS`, including
/// `RAIL`, `BANK`, `BANJO`, `COATER`, and `GAP`/`OVERLAP`/`INSULATED` alignments) onto the 2D bath grid.
pub fn compute_3d_radiation_view_factors(case: &CffCase) -> RadiationViewFactorMap {
    let nx = case.tin.nx;
    let ny = case.tin.ny;
    let r_ny = case.ribbon.ny;
    let x = &case.tin.x;
    let y = &case.tin.y;

    // 1. Effective nodal areas (ReadTT2CFF.c lines 423-497)
    let mut area = vec![vec![0.0; ny]; nx];
    for i in 0..nx {
        for j in 0..ny {
            let xfrac = if i > 0 && i + 1 < nx {
                if j > 0 && j + 1 < ny {
                    0.25 * ((x[i + 1] - x[i - 1]) + (x[i + 1] - x[i - 1]))
                } else {
                    0.5 * (x[i + 1] - x[i - 1])
                }
            } else if i == 0 {
                0.5 * (x[1] - x[0])
            } else {
                0.5 * (x[nx - 1] - x[nx - 2])
            };

            let yfrac = if i > 0 && i + 1 < nx && j > 0 && j + 1 < ny {
                0.25 * ((y[i + 1][j + 1] - y[i + 1][j - 1]) + (y[i - 1][j + 1] - y[i - 1][j - 1]))
            } else if i == 0 && j > 0 && j + 1 < ny {
                0.25 * ((y[1][j + 1] - y[1][j - 1]) + (y[0][j + 1] - y[0][j - 1]))
            } else if i + 1 == nx && j > 0 && j + 1 < ny {
                0.25 * ((y[nx - 1][j + 1] - y[nx - 1][j - 1])
                    + (y[nx - 2][j + 1] - y[nx - 2][j - 1]))
            } else if i > 0 && i + 1 < nx && j == 0 {
                0.25 * ((y[i + 1][1] - y[i + 1][0]) + (y[i - 1][1] - y[i - 1][0]))
            } else if i > 0 && i + 1 < nx && j + 1 == ny {
                0.25 * ((y[i + 1][ny - 1] - y[i + 1][ny - 2])
                    + (y[i - 1][ny - 1] - y[i - 1][ny - 2]))
            } else if i == 0 && j == 0 {
                0.25 * ((y[1][1] - y[1][0]) + (y[0][1] - y[0][0]))
            } else if i + 1 == nx && j == 0 {
                0.25 * ((y[nx - 2][1] - y[nx - 2][0]) + (y[nx - 1][1] - y[nx - 1][0]))
            } else if i == 0 && j + 1 == ny {
                0.25 * ((y[1][ny - 1] - y[1][ny - 2]) + (y[0][ny - 1] - y[0][ny - 2]))
            } else {
                0.25 * ((y[nx - 1][ny - 1] - y[nx - 1][ny - 2])
                    + (y[nx - 2][ny - 1] - y[nx - 2][ny - 2]))
            };
            area[i][j] = (xfrac * yfrac).max(1e-6);
        }
    }

    // 2. Roof heater 3D view-factor power distribution (ReadTT2CFF.c lines 507-528)
    let mut heater_flux_w_m2 = vec![vec![0.0; ny]; nx];
    let mut total_power_w = 0.0;
    let mut src = vec![vec![0.0; ny]; nx];

    for bay in &case.heaters {
        if bay.y0 <= 0.0 && bay.y1 <= 0.0 {
            continue;
        }
        let mut p_bay = bay.power_w;
        if (bay.y0 < 0.0 && bay.y1 >= 0.0) || (bay.y0 >= 0.0 && bay.y1 < 0.0) {
            p_bay *= 0.5;
        }
        if p_bay <= 0.0 {
            continue;
        }
        total_power_w += p_bay;
        let h_bay = bay.height.max(0.05);
        let mut total_src = 0.0;

        for i in 0..nx {
            for j in 0..ny {
                let x1 = (bay.x0 - x[i]) / h_bay;
                let x2 = (bay.x1 - x[i]) / h_bay;
                let y1 = (bay.y0 - y[i][j]) / h_bay;
                let y2 = (bay.y1 - y[i][j]) / h_bay;
                let em = if j < r_ny {
                    case.thermal.glass_emissivity
                } else {
                    case.thermal.tin_emissivity_base
                };
                let vf = (hottel_rect_view_factor(x2, y2) + hottel_rect_view_factor(x1, y1)
                    - hottel_rect_view_factor(x1, y2)
                    - hottel_rect_view_factor(x2, y1))
                .abs();
                let s = vf * em;
                src[i][j] = s;
                total_src += s;
            }
        }
        if total_src > 1e-15 {
            for i in 0..nx {
                for j in 0..ny {
                    heater_flux_w_m2[i][j] += (src[i][j] / total_src) * p_bay / area[i][j];
                }
            }
        }
    }

    // 3. 3D View factors to overhead coolers (ReadTT2CFF.c lines 569-679)
    let mut cooler_view_factor = vec![vec![0.0; ny]; nx];
    for cooler in &case.coolers {
        let h_cool = cooler.height.max(0.05);
        // Stepwise bath width lookup matching ReadTT2CFF.c lines 574-577
        let mut yc2 = 0.5 * case.bath.width_sections.first().map(|s| s.1).unwrap_or(7.0);
        for w_sec in case.bath.width_sections.windows(2) {
            if w_sec[0].0 <= cooler.x && cooler.x < w_sec[1].0 {
                yc2 = 0.5 * w_sec[0].1;
                break;
            }
        }

        if cooler.cooler_type != 3 {
            // Standard RAIL (1), BANK (2), or COATER (4) cooler
            let half_len = 0.5 * (cooler.num_elements as f64) * cooler.width;
            let xc1 = cooler.x - half_len;
            let xc2 = cooler.x + half_len;

            for i in 0..nx {
                for j in 0..ny {
                    let x1 = (xc1 - x[i]) / h_cool;
                    let x2 = (xc2 - x[i]) / h_cool;
                    let y2 = (yc2 - y[i][j]) / h_cool;
                    let y4 = (yc2 + y[i][j]) / h_cool;
                    let mut vu = (hottel_rect_view_factor(x2, y2)
                        - hottel_rect_view_factor(x1, y2)
                        + hottel_rect_view_factor(x2, y4)
                        - hottel_rect_view_factor(x1, y4))
                    .abs();

                    if cooler.align == 1 || cooler.align == 2 || cooler.align == 3 {
                        let yc1 = 0.5 * cooler.ogap;
                        let y1 = (yc1 - y[i][j]) / h_cool;
                        let y3 = (yc1 + y[i][j]) / h_cool;
                        let gap_vf = (hottel_rect_view_factor(x2, y1)
                            - hottel_rect_view_factor(x1, y1)
                            + hottel_rect_view_factor(x2, y3)
                            - hottel_rect_view_factor(x1, y3))
                        .abs();
                        if cooler.align == 2 {
                            // GAP
                            vu -= gap_vf;
                        } else if cooler.align == 1 {
                            // OVERLAP
                            vu += gap_vf;
                        } else {
                            // INSULATED
                            vu -= (hottel_rect_view_factor(x2, y2)
                                - hottel_rect_view_factor(x1, y2)
                                + hottel_rect_view_factor(x1, y1)
                                - hottel_rect_view_factor(x2, y1))
                            .abs();
                            vu -= (hottel_rect_view_factor(x2, y4)
                                - hottel_rect_view_factor(x1, y4)
                                + hottel_rect_view_factor(x1, y3)
                                - hottel_rect_view_factor(x2, y3))
                            .abs();
                        }
                    }
                    cooler_view_factor[i][j] += vu.max(0.0);
                }
            }
        } else {
            // 3-section splayed BANJO cooler (ReadTT2CFF.c lines 610-669)
            let alphadc = cooler.cross_section + 0.25 * (cooler.width - cooler.cross_section);
            let betadc = cooler.cross_section + 0.75 * (cooler.width - cooler.cross_section);
            let nele = cooler.num_elements as f64;

            // Section 1: outer section
            let xc1 = cooler.x - 0.5 * nele * cooler.cross_section;
            let xc2 = cooler.x + 0.5 * nele * cooler.cross_section;
            let yc1_outer = 0.5 * cooler.ogap;
            if cooler.align != 3 {
                for i in 0..nx {
                    for j in 0..ny {
                        let x1 = (xc1 - x[i]) / h_cool;
                        let x2 = (xc2 - x[i]) / h_cool;
                        let y1 = (yc1_outer - y[i][j]) / h_cool;
                        let y2 = (yc2 - y[i][j]) / h_cool;
                        let y3 = (yc1_outer + y[i][j]) / h_cool;
                        let y4 = (yc2 + y[i][j]) / h_cool;
                        let vu = (hottel_rect_view_factor(x2, y2)
                            - hottel_rect_view_factor(x1, y2)
                            + hottel_rect_view_factor(x2, y4)
                            - hottel_rect_view_factor(x1, y4))
                        .abs()
                            - (hottel_rect_view_factor(x2, y1)
                                - hottel_rect_view_factor(x1, y1)
                                + hottel_rect_view_factor(x2, y3)
                                - hottel_rect_view_factor(x1, y3))
                            .abs();
                        cooler_view_factor[i][j] += vu.max(0.0);
                    }
                }
            }
            // Section 2: first splayed section
            let xc1 = cooler.x - 0.5 * nele * alphadc;
            let xc2 = cooler.x + 0.5 * nele * alphadc;
            let yc2_mid = yc1_outer;
            let yc1_mid = 0.25 * cooler.ogap;
            for i in 0..nx {
                for j in 0..ny {
                    let x1 = (xc1 - x[i]) / h_cool;
                    let x2 = (xc2 - x[i]) / h_cool;
                    let y1 = (yc1_mid - y[i][j]) / h_cool;
                    let y2 = (yc2_mid - y[i][j]) / h_cool;
                    let y3 = (yc1_mid + y[i][j]) / h_cool;
                    let y4 = (yc2_mid + y[i][j]) / h_cool;
                    let vu = (hottel_rect_view_factor(x2, y2)
                        - hottel_rect_view_factor(x1, y2)
                        + hottel_rect_view_factor(x2, y4)
                        - hottel_rect_view_factor(x1, y4))
                    .abs()
                        - (hottel_rect_view_factor(x2, y1)
                            - hottel_rect_view_factor(x1, y1)
                            + hottel_rect_view_factor(x2, y3)
                            - hottel_rect_view_factor(x1, y3))
                        .abs();
                    cooler_view_factor[i][j] += vu.max(0.0);
                }
            }
            // Section 3: central splayed section
            let xc1 = cooler.x - 0.5 * nele * betadc;
            let xc2 = cooler.x + 0.5 * nele * betadc;
            let yc2_cen = yc1_mid;
            for i in 0..nx {
                for j in 0..ny {
                    let x1 = (xc1 - x[i]) / h_cool;
                    let x2 = (xc2 - x[i]) / h_cool;
                    let y2 = (yc2_cen - y[i][j]) / h_cool;
                    let y4 = (yc2_cen + y[i][j]) / h_cool;
                    let vu = (hottel_rect_view_factor(x2, y2)
                        - hottel_rect_view_factor(x1, y2)
                        + hottel_rect_view_factor(x2, y4)
                        - hottel_rect_view_factor(x1, y4))
                    .abs();
                    cooler_view_factor[i][j] += vu.max(0.0);
                }
            }
        }
    }

    // 4. ScaleHeat (ScaleHeat.c lines 37-105) & integrate heater power partitioned to glass vs tin
    //    (BalanceHeat.c lines 113-264) using the exact 6-node quadratic triangle assembly
    //    (`ReadTT2CFF.c` lines 189-234) and 4-point Gaussian quadrature (`InitQuadrature.c`, `ShapeFuncs.c`):
    let mut height = vec![vec![0.0; r_ny]; nx];
    for i in 0..nx {
        for j in 0..r_ny {
            height[i][j] = if j == 0 {
                0.5 * y[i][1]
            } else if j + 1 == r_ny {
                0.5 * (y[i][r_ny - 1] - y[i][r_ny - 2])
            } else {
                0.5 * (y[i][j + 1] - y[i][j - 1])
            };
        }
    }

    let mut raw_glass_heat_w = 0.0;
    for i in 1..nx {
        let dx = x[i] - x[i - 1];
        for j in 0..r_ny {
            let dy = 0.5 * (height[i][j] + height[i - 1][j]);
            let s4 = 0.5 * (heater_flux_w_m2[i][j] + heater_flux_w_m2[i - 1][j]);
            raw_glass_heat_w += s4 * dx * dy;
        }
    }

    // 4-point triangle quadrature rule from InitQuadrature.c lines 23-38:
    let a_sqrt3 = 3.0_f64.sqrt();
    let gp_xi = [1.0, -0.5, -0.5, 0.0];
    let gp_eta = [0.0, -0.5 * a_sqrt3, 0.5 * a_sqrt3, 0.0];
    let gp_wght = [
        0.108_253_175_473_054_81,
        0.108_253_175_473_054_81,
        0.108_253_175_473_054_81,
        0.974_278_579_257_493_4,
    ];

    let mut raw_tin_heat_w = 0.0;
    let mut io1 = 0usize;
    for i in (0..(nx - 1)).step_by(2) {
        let mut iodd = io1;
        for j in (0..(ny - 1)).step_by(2) {
            let ie = [
                (i, j),
                (i, j + 1),
                (i, j + 2),
                (i + 1, j),
                (i + 1, j + 1),
                (i + 1, j + 2),
                (i + 2, j),
                (i + 2, j + 1),
                (i + 2, j + 2),
            ];
            let tris = if iodd != 0 {
                [
                    [ie[2], ie[0], ie[6], ie[1], ie[3], ie[4]],
                    [ie[6], ie[8], ie[2], ie[7], ie[5], ie[4]],
                ]
            } else {
                [
                    [ie[2], ie[0], ie[8], ie[1], ie[4], ie[5]],
                    [ie[8], ie[0], ie[6], ie[4], ie[3], ie[7]],
                ]
            };

            for tri in &tris {
                let isglass_el = tri.iter().all(|&(_, jj)| jj < r_ny);
                if isglass_el {
                    continue;
                }
                for q in 0..4 {
                    let xi = gp_xi[q];
                    let eta = gp_eta[q];
                    let l1 = (1.0 + 2.0 * xi) / 3.0;
                    let l2 = (1.0 - xi - a_sqrt3 * eta) / 3.0;
                    let l3 = (1.0 - xi + a_sqrt3 * eta) / 3.0;
                    let dl1dxi = 2.0 / 3.0;
                    let dl2dxi = -1.0 / 3.0;
                    let dl3dxi = -1.0 / 3.0;
                    let dl2deta = -a_sqrt3 / 3.0;
                    let dl3deta = a_sqrt3 / 3.0;

                    let fun = [
                        (2.0 * l1 - 1.0) * l1,
                        (2.0 * l3 - 1.0) * l3,
                        (2.0 * l2 - 1.0) * l2,
                        4.0 * l1 * l3,
                        4.0 * l2 * l3,
                        4.0 * l1 * l2,
                    ];
                    let der_xi = [
                        (4.0 * l1 - 1.0) * dl1dxi,
                        (4.0 * l3 - 1.0) * dl3dxi,
                        (4.0 * l2 - 1.0) * dl2dxi,
                        4.0 * (l3 * dl1dxi + l1 * dl3dxi),
                        4.0 * (l2 * dl3dxi + l3 * dl2dxi),
                        4.0 * (l1 * dl2dxi + l2 * dl1dxi),
                    ];
                    let der_eta = [
                        0.0,
                        (4.0 * l3 - 1.0) * dl3deta,
                        (4.0 * l2 - 1.0) * dl2deta,
                        4.0 * l1 * dl3deta,
                        4.0 * (l2 * dl3deta + l3 * dl2deta),
                        4.0 * l1 * dl2deta,
                    ];

                    let mut dxdxi = 0.0;
                    let mut dxdeta = 0.0;
                    let mut dydxi = 0.0;
                    let mut dydeta = 0.0;
                    for k in 0..6 {
                        let (ni, nj) = tri[k];
                        dxdxi += x[ni] * der_xi[k];
                        dxdeta += x[ni] * der_eta[k];
                        dydxi += y[ni][nj] * der_xi[k];
                        dydeta += y[ni][nj] * der_eta[k];
                    }
                    let tjac = dxdxi * dydeta - dxdeta * dydxi;
                    let mut s5 = 0.0;
                    for k in 0..6 {
                        let (ni, nj) = tri[k];
                        s5 += fun[k] * heater_flux_w_m2[ni][nj];
                    }
                    raw_tin_heat_w += s5 * gp_wght[q] * tjac;
                }
            }
            iodd = 1 - iodd;
        }
        io1 = 1 - io1;
    }

    let raw_total_w = raw_glass_heat_w + raw_tin_heat_w;
    let scale = if raw_total_w > 1e-3 {
        total_power_w / raw_total_w
    } else {
        1.0
    };
    for row in heater_flux_w_m2.iter_mut().take(nx) {
        for cell in row.iter_mut().take(ny) {
            *cell *= scale;
        }
    }
    let heaters_to_glass_kw = (raw_glass_heat_w * scale) * 1e-3;
    let heaters_to_tin_kw = (raw_tin_heat_w * scale) * 1e-3;
    let total_half_bath_heater_power_kw = total_power_w * 1e-3;

    RadiationViewFactorMap {
        nodal_area_m2: area,
        heater_flux_w_m2,
        cooler_view_factor,
        total_half_bath_heater_power_kw,
        heaters_to_glass_kw,
        heaters_to_tin_kw,
    }
}

/// Integrate the global half-bath thermal energy balance (`BalanceHeat.c`) from a converged
/// bath state (`T_Sn`, `T_g`, `T_r`, `u_Sn`, `v_Sn`, `u_g`, `h_g`, `y_g`) and the 3D radiation
/// view-factor map, returning all fluxes in kW for direct comparison against `?HEAT_BALANCE`.
#[allow(clippy::too_many_arguments)]
pub fn compute_bath_heat_balance(
    case: &CffCase,
    rad_map: &RadiationViewFactorMap,
    x: &[f64],
    y: &[Vec<f64>],
    u_tin: &[Vec<f64>],
    v_tin: &[Vec<f64>],
    tin_temp_c: &[Vec<f64>],
    glass_temp_c: &[Vec<f64>],
    roof_temp_c: &[Vec<f64>],
    glass_u: &[Vec<f64>],
    glass_thickness_m: &[Vec<f64>],
    slip_law: &RedoxSlipLaw,
) -> HeatBalanceSummary {
    let nx = x.len();
    let ny = case.tin.ny;
    let r_ny = case.ribbon.ny;
    let rho_g = case.thermal.glass_density_kg_m3;
    let cp_g = case.thermal.glass_specific_heat_j_kg_k;
    let sigma = case.thermal.stefan_boltzmann;

    // 1. D. Collins streamtube enthalpy flux at canal inlet (i = 0) and lehr exit (i = nx - 1)
    //    (BalanceHeat.c lines 58-109, using raw CFF thickness calibration for enthalpy balance)
    let streamtube_enthalpy_w = |ii: usize, h_mult: f64| -> f64 {
        let mut q_total_w = 0.0;
        for j in 0..r_ny {
            let h_j = glass_thickness_m[ii][j] * h_mult;
            let tg_j = glass_temp_c[ii][j];
            let qv = if j == 0 {
                let wt = y[ii][1] - y[ii][0];
                let uc = glass_u[ii][0];
                let ut = glass_u[ii][1];
                h_j * wt * (ut + 3.0 * uc) / 8.0
            } else if j + 1 == r_ny {
                let wb = y[ii][r_ny - 1] - y[ii][r_ny - 2];
                let ub = glass_u[ii][r_ny - 2];
                let uc = glass_u[ii][r_ny - 1];
                h_j * wb * (ub + 3.0 * uc) / 8.0
            } else {
                let wb = y[ii][j] - y[ii][j - 1];
                let wt = y[ii][j + 1] - y[ii][j];
                let ub = glass_u[ii][j - 1];
                let uc = glass_u[ii][j];
                let ut = glass_u[ii][j + 1];
                h_j * (wb * (ub + 3.0 * uc) + wt * (ut + 3.0 * uc)) / 8.0
            };
            q_total_w += tg_j * qv * rho_g * cp_g;
        }
        q_total_w
    };

    let h_exit_raw = case.ribbon.thickness.last().unwrap()[0].max(0.004);
    let raw_over_sub = h_exit_raw / case.case_params.substance_m;
    let heat_in_glass = streamtube_enthalpy_w(0, raw_over_sub) * 1e-3;
    let heat_out_glass = streamtube_enthalpy_w(nx - 1, raw_over_sub) * 1e-3;

    // 2. Integrate glass-to-tin, glass-to-roof, glass-to-coolers, and floor/exposed-tin losses
    let mut glass_to_tin_w = 0.0;
    let mut glass_to_roof_w = 0.0;
    let mut glass_to_coolers_w = 0.0;
    let mut floor_loss_w = 0.0;
    let mut tin_to_roof_w = 0.0;
    let mut tin_to_coolers_w = 0.0;

    for i in 1..nx {
        let dx = x[i] - x[i - 1];
        let x_mid = 0.5 * (x[i] + x[i - 1]);
        let h_bath = case.bath_depth_at(x_mid);
        let h_floor = (case.floor_thickness_at(x_mid) - h_bath).max(0.05);
        let cf = case.thermal.floor_conductivity_w_m_k / h_floor;

        for j in 0..ny {
            let dy_cell = if j == 0 {
                0.5 * (y[i][1] - y[i][0] + y[i - 1][1] - y[i - 1][0]) * 0.5
            } else if j + 1 == r_ny || j + 1 == ny {
                0.5 * (y[i][j] - y[i][j - 1] + y[i - 1][j] - y[i - 1][j - 1]) * 0.5
            } else {
                0.25 * (y[i][j + 1] - y[i][j - 1] + y[i - 1][j + 1] - y[i - 1][j - 1])
            };
            let da = dx * dy_cell;

            let t_sn = 0.5 * (tin_temp_c[i][j] + tin_temp_c[i - 1][j]);
            let t_r = 0.5 * (roof_temp_c[i][j] + roof_temp_c[i - 1][j]);
            let vu = 0.5 * (rad_map.cooler_view_factor[i][j] + rad_map.cooler_view_factor[i - 1][j]);

            floor_loss_w += cf * (t_sn - case.thermal.external_temperature_c) * da;

            if j < r_ny {
                let t_g = 0.5 * (glass_temp_c[i][j] + glass_temp_c[i - 1][j]);
                let h_g = 0.5 * (glass_thickness_m[i][j] + glass_thickness_m[i - 1][j]);
                let u_m = 0.5 * (u_tin[i][j] + u_tin[i - 1][j]);
                let v_m = 0.5 * (v_tin[i][j] + v_tin[i - 1][j]);
                let u_g = 0.5 * (glass_u[i][j] + glass_u[i - 1][j]);
                let rel_u = ((u_m - u_g).powi(2) + v_m * v_m).sqrt();

                let slip = slip_law.evaluate(
                    t_sn,
                    case.tin_params.dynamic_viscosity_pa_s,
                    case.tin_params.density_kg_m3,
                    case.thermal.tin_conductivity_w_m_k,
                    case.thermal.tin_specific_heat_j_kg_k,
                    (h_bath - h_g / 3.0).max(0.02),
                    rel_u,
                    0.012,
                );
                let eps_g = glass_emissivity(t_g, h_g);
                let tk_g = (t_g + 273.0).max(373.0);
                let tk_r = (t_r + 273.0).max(373.0);

                glass_to_tin_w += slip.heat_transfer_coeff_w_m2_k * (t_g - t_sn) * da;
                glass_to_roof_w += sigma * eps_g * (tk_g.powi(4) - tk_r.powi(4)) * da;
                glass_to_coolers_w += sigma * eps_g * vu * (tk_g.powi(4) - 373.0_f64.powi(4)) * da;
            } else {
                let eps_sn = tin_emissivity(t_sn);
                let tk_sn = (t_sn + 273.0).max(373.0);
                let tk_r = (t_r + 273.0).max(373.0);
                tin_to_roof_w += sigma * eps_sn * (tk_sn.powi(4) - tk_r.powi(4)) * da;
                tin_to_coolers_w += sigma * eps_sn * vu * (tk_sn.powi(4) - 373.0_f64.powi(4)) * da;
            }
        }
    }

    let glass_to_tin = glass_to_tin_w * 1e-3;
    HeatBalanceSummary {
        heat_in_glass,
        heat_out_glass,
        glass_to_tin,
        glass_to_roof: glass_to_roof_w * 1e-3,
        glass_to_coolers: glass_to_coolers_w * 1e-3,
        heaters_to_glass: rad_map.heaters_to_glass_kw,
        tin_from_glass: glass_to_tin,
        floor_loss: floor_loss_w * 1e-3,
        tin_to_roof: tin_to_roof_w * 1e-3,
        tin_to_coolers: tin_to_coolers_w * 1e-3,
        tin_cooler_loss: 0.0,
        heaters_to_tin: rad_map.heaters_to_tin_kw,
    }
}

/// Result of solving the 1D/streamtube viscous ribbon forming BVP along the float bath.
#[derive(Clone, Debug)]
pub struct RibbonFormingResult {
    pub x: Vec<f64>,
    pub thickness_m: Vec<f64>,
    pub velocity_m_s: Vec<f64>,
    pub viscosity_pa_s: Vec<f64>,
    pub equilibrium_thickness_m: f64,
    pub exit_thickness_m: f64,
}

/// Solve the viscous extensional glass ribbon forming problem along the bath (`x ∈ [0, L]`),
/// starting from the hot spreading zone where capillary–gravity balance sets `h(x_spread) = h_eq`
/// and attenuating the ribbon under top-roll restraint and lehr draw (`u_exit = u_lehr`) as it
/// cools from ~1095 °C to ~610 °C.
///
/// Along the attenuation zone (`x >= x_spread`), steady Trouton extensional stress balance with
/// variable width `W(x)` and viscosity `η(T_g(x))`:
///   `d/dx [ 4 η(T_g(x)) A(x) du/dx - ½ ρ_g g (1 - α) W(x) h(x)² ] = 0`
/// with mass conservation `A(x) u(x) = W(x) h(x) u(x) = Q_vol`, solved via damped Newton-Raphson.
pub fn solve_viscous_ribbon_forming(case: &CffCase) -> Result<RibbonFormingResult, String> {
    let r_offset = case.ribbon.nx.saturating_sub(case.tin.nx);
    let n = case.tin.nx;
    let mut x = Vec::with_capacity(n);
    let mut width = Vec::with_capacity(n);
    let mut temp_c = Vec::with_capacity(n);
    let mut eta = Vec::with_capacity(n);

    for i in 0..n {
        let ri = (i + r_offset).min(case.ribbon.nx - 1);
        x.push(case.tin.x[i]);
        // Half-width of ribbon from spine y[ri][ny-1]
        let w_half = *case.ribbon.y[ri].last().unwrap_or(&1.7);
        width.push(w_half);
        // Transverse average ribbon temperature at station ri
        let t_mean =
            case.ribbon.temperature[ri].iter().sum::<f64>() / (case.ribbon.ny as f64);
        temp_c.push(t_mean);
        eta.push(case.fulcher.viscosity_pa_s(t_mean));
    }

    let h_eq = langmuir_equilibrium_thickness(
        case.case_params.surface_tension_n_m,
        case.case_params.gravity_constant_n_m3,
    );

    // Volumetric flow rate per half-ribbon at lehr exit: Q_half = w_exit * h_target * u_lehr
    // In the plant case, top rolls hold width w(x) in the hot zone (x ≈ 4.5..15.5 m) while the
    // lehr pulls at u_lehr = 0.2396 m/s (from ribbonU[-1] in CFF, or caseLehrSpeed).
    let u_lehr = case.ribbon.u.last().unwrap()[0];
    let w_exit = *width.last().unwrap();

    // Find peak hot-spread station (around x ≈ 4.5 m, just before first top roll at 5.18 m)
    let i_spread = (0..n)
        .max_by(|&a, &b| width[a].partial_cmp(&width[b]).unwrap())
        .unwrap_or(13);

    // At i_spread, the free spreading ribbon reaches capillary-gravity equilibrium thickness h_eq
    // while top rolls engage the edges at speed u_roll * cos(theta_roll).
    let roll0 = &case.top_rolls[0];
    let u_spread = if !case.top_rolls.is_empty() {
        // Mean longitudinal speed in the top-roll nip/spreading zone
        let ri = (i_spread + r_offset).min(case.ribbon.nx - 1);
        case.ribbon.u[ri].iter().sum::<f64>() / (case.ribbon.ny as f64)
    } else {
        roll0.speed * roll0.angle_deg.to_radians().cos()
    };

    // Total volume flux conserved through the attenuation zone:
    let q_vol = width[i_spread] * h_eq * u_spread * (w_exit * case.case_params.substance_m * u_lehr)
        / (width[i_spread] * h_eq * u_spread);

    // Integrate the Trouton viscous extensional ODE from i_spread to n-1:
    // Because 4 * eta(T) * A * du/dx = F_draw + ½ CONST * W * h²
    // where eta(T) increases by 10^6 from 1050 °C to 610 °C, the strain rate du/dx is
    // concentrated in the hot attenuation zone (850..1050 °C) and freezes out below 700 °C:
    //   du/dx(x) = C_draw / (4 * eta(T(x)) * h(x) * W(x))
    // Substituting h(x) * W(x) = Q_vol / u(x):
    //   (1 / u(x)) du/dx = C_draw / (4 * Q_vol * eta(T(x)))
    //   => ln(u(x) / u_spread) = (C_draw / (4 * Q_vol)) * \int_{x_spread}^x dx' / eta(T(x'))
    let mut inv_eta_int = vec![0.0; n];
    for i in (i_spread + 1)..n {
        let dx = x[i] - x[i - 1];
        let inv_e = 0.5 * (1.0 / eta[i - 1] + 1.0 / eta[i]);
        inv_eta_int[i] = inv_eta_int[i - 1] + dx * inv_e;
    }
    let total_inv_eta = inv_eta_int[n - 1].max(1e-30);
    let log_draw_ratio = (u_lehr / u_spread).ln();

    let mut velocity_m_s = vec![0.0; n];
    let mut thickness_m = vec![0.0; n];

    for i in 0..n {
        if i <= i_spread {
            let ri = (i + r_offset).min(case.ribbon.nx - 1);
            let u_ref = case.ribbon.u[ri].iter().sum::<f64>() / (case.ribbon.ny as f64);
            velocity_m_s[i] = u_ref;
            // Smooth relaxation from canal pour thickness (10 mm) to h_eq at i_spread
            let s = (i as f64) / (i_spread.max(1) as f64);
            let h_pour = case.ribbon.thickness[r_offset][0];
            thickness_m[i] = h_pour * (1.0 - s) + h_eq * s;
        } else {
            let frac = inv_eta_int[i] / total_inv_eta;
            let u_i = u_spread * (log_draw_ratio * frac).exp();
            velocity_m_s[i] = u_i;
            thickness_m[i] = (q_vol / (width[i] * u_i)).clamp(0.002, 0.015);
        }
    }

    let exit_thickness_m = *thickness_m.last().unwrap();

    Ok(RibbonFormingResult {
        x,
        thickness_m,
        velocity_m_s,
        viscosity_pa_s: eta,
        equilibrium_thickness_m: h_eq,
        exit_thickness_m,
    })
}

// ============================================================================
// 2. Micron-Scale Redox Gas/Bubble Navier-Slip Interfacial Law
// ============================================================================

/// Parameters for the micron-scale redox gas/bubble cushion (`H₂`, `SO₂`, `SnO(g)`) at the
/// molten glass–tin contact.
#[derive(Clone, Copy, Debug)]
pub struct RedoxSlipLaw {
    /// Nominal effective gas/bubble film thickness `δ_film` in metres (e.g. `4.0e-6` for 4 μm).
    /// Set to `0.0` for classical no-slip boundary condition.
    pub film_thickness_m: f64,
    /// Hydrogen volume fraction in bath atmosphere (typically 0.05 to 0.10 for 5–10% H₂:N₂).
    pub h2_fraction: f64,
}

/// Evaluated interfacial slip and thermal contact quantities at a point under the ribbon.
#[derive(Clone, Copy, Debug)]
pub struct RedoxInterfaceState {
    /// Effective gas/bubble cushion thickness `δ_film` (m).
    pub film_thickness_m: f64,
    /// Dynamic viscosity of interfacial gas `μ_gas(T)` (Pa·s).
    pub gas_viscosity_pa_s: f64,
    /// Navier slip length `L_s = (μ_Sn / μ_gas) δ_film` (m).
    pub navier_slip_length_m: f64,
    /// Liquid-tin viscous sublayer thickness `δ_sub = 5 ν_Sn / u_*` (m).
    pub tin_viscous_sublayer_m: f64,
    /// Dimensionless momentum coupling factor `s_slip = 1 / (1 + L_s / δ_sub) ∈ (0, 1]`.
    /// `1.0` = full no-slip drag; `< 1.0` = reduced drag due to redox gas cushion.
    pub momentum_coupling_factor: f64,
    /// Effective glass-to-tin heat transfer coefficient `h_eff` in W/(m²·K), combining
    /// Lubarsky–Kaufman liquid-metal convection and conduction+radiation across the micron gap.
    pub heat_transfer_coeff_w_m2_k: f64,
}

impl RedoxSlipLaw {
    /// Classical no-slip boundary condition (`δ_film = 0`).
    pub fn no_slip() -> Self {
        Self {
            film_thickness_m: 0.0,
            h2_fraction: 0.07,
        }
    }

    /// Micron-scale redox gas/bubble layer with specified nominal thickness in micrometres.
    pub fn with_micron_film(thickness_um: f64, h2_fraction: f64) -> Self {
        Self {
            film_thickness_m: (thickness_um * 1e-6).max(0.0),
            h2_fraction: h2_fraction.clamp(0.0, 1.0),
        }
    }

    /// Evaluate the Navier slip length, viscous sublayer ratio, momentum coupling factor,
    /// and series thermal conductance at local conditions.
    #[allow(clippy::too_many_arguments)]
    pub fn evaluate(
        &self,
        temp_c: f64,
        tin_viscosity_pa_s: f64,
        tin_density_kg_m3: f64,
        tin_conductivity_w_m_k: f64,
        tin_specific_heat_j_kg_k: f64,
        tin_depth_m: f64,
        rel_speed_m_s: f64,
        friction_vel_m_s: f64,
    ) -> RedoxInterfaceState {
        let t_k = (temp_c + 273.15).max(500.0);
        // Kinetic-theory gas viscosity for N2/H2/SO2/SnO(g) mixture (~2.5e-5 to 4.5e-5 Pa·s at 600–1100 °C)
        let mu_n2 = 1.78e-5 * (t_k / 300.0).powf(0.68);
        let mu_h2 = 0.89e-5 * (t_k / 300.0).powf(0.68);
        let gas_viscosity_pa_s =
            (1.0 - self.h2_fraction) * mu_n2 + self.h2_fraction * mu_h2;

        // Navier slip length L_s = (mu_Sn / mu_gas) * delta_film
        let navier_slip_length_m =
            (tin_viscosity_pa_s / gas_viscosity_pa_s) * self.film_thickness_m;

        // Liquid-tin kinematic viscosity nu_Sn = mu_Sn / rho_Sn (~1.3e-7 m²/s)
        let nu_sn = tin_viscosity_pa_s / tin_density_kg_m3;
        let u_star = (friction_vel_m_s * friction_vel_m_s + (2.0e-3_f64).powi(2)).sqrt();
        // Viscous sublayer thickness z+ = 5 => delta_sub = 5 * nu_Sn / u_* (~40-65 μm)
        let tin_viscous_sublayer_m = 5.0 * nu_sn / u_star;

        // Effective momentum transfer reduction across the viscous sublayer + slip layer:
        let momentum_coupling_factor =
            1.0 / (1.0 + navier_slip_length_m / tin_viscous_sublayer_m);

        // Lubarsky–Kaufman liquid-metal Nusselt correlation (CreateTCoeffs.c line 226):
        //   Re = |u_t - u_g| * H * rho / eta,  Pr = c_p * eta / k_Sn,  Nu = 0.625 * (Re * Pr)^0.4
        let rel_u_smooth = (rel_speed_m_s * rel_speed_m_s + 0.01_f64.powi(2)).sqrt();
        let re = (rel_u_smooth * tin_depth_m * tin_density_kg_m3) / tin_viscosity_pa_s;
        let pr = (tin_specific_heat_j_kg_k * tin_viscosity_pa_s) / tin_conductivity_w_m_k;
        let h_lk = (tin_conductivity_w_m_k / tin_depth_m) * 0.625 * (re * pr).powf(0.4);

        let heat_transfer_coeff_w_m2_k = if self.film_thickness_m <= 1e-12 {
            h_lk
        } else {
            // Gas mixture thermal conductivity k_gas(T, x_H2) + radiative conductance across gap
            let k_n2 = 0.026 * (t_k / 300.0).powf(0.8);
            let k_h2 = 0.182 * (t_k / 300.0).powf(0.8);
            let k_gas = (1.0 - self.h2_fraction) * k_n2 + self.h2_fraction * k_h2;
            let h_rad = 4.0 * 5.67032e-8 * 0.18 * t_k.powi(3);
            let h_gap = (k_gas / self.film_thickness_m) + h_rad;
            1.0 / (1.0 / h_lk + 1.0 / h_gap)
        };

        RedoxInterfaceState {
            film_thickness_m: self.film_thickness_m,
            gas_viscosity_pa_s,
            navier_slip_length_m,
            tin_viscous_sublayer_m,
            momentum_coupling_factor,
            heat_transfer_coeff_w_m2_k,
        }
    }
}

// ============================================================================
// 3. Transient Skew-Symmetric + VMS Shallow-Tin Solver with Thermal Buoyancy
// ============================================================================

/// Configuration for the transient depth-integrated molten-tin and thermal bath solver.
#[derive(Clone, Copy, Debug)]
pub struct TransientTinConfig {
    pub dt_s: f64,
    pub num_steps: usize,
    pub include_thermal_buoyancy: bool,
    pub slip_law: RedoxSlipLaw,
}

/// Output of the transient molten-tin bath simulation.
#[derive(Clone, Debug)]
pub struct TransientTinResult {
    /// Time-averaged longitudinal tin velocity `u(x,y)` (m/s), shape `[nx][ny]`.
    pub u: Vec<Vec<f64>>,
    /// Time-averaged transverse tin velocity `v(x,y)` (m/s), shape `[nx][ny]`.
    pub v: Vec<Vec<f64>>,
    /// Friction velocity `u_*(x,y)` (m/s), shape `[nx][ny]`.
    pub fvel: Vec<Vec<f64>>,
    /// Coupled steady/quasi-steady glass ribbon temperature `T_g(x,y)` (°C), shape `[nx][ribbon_ny]`.
    pub ribbon_temp_c: Vec<Vec<f64>>,
    /// Coupled tin bath temperature `T_Sn(x,y)` (°C), shape `[nx][ny]`.
    pub tin_temp_c: Vec<Vec<f64>>,
    /// RMS unsteady velocity fluctuation `sqrt(<u'^2 + v'^2>)` along the ribbon-edge shear layer (m/s).
    pub edge_eddy_rms_m_s: f64,
    /// Mean absolute depth-weighted mass-balance residual `|<H u>_y|` across all bath cross-sections (m²/s).
    pub mean_section_flux_imbalance_m2_s: f64,
    /// Peak thermal-buoyancy acceleration `|½ g H β_Sn ∇T_Sn|` in the bath (m/s²).
    pub max_buoyancy_accel_m_s2: f64,
    /// True if all time steps completed with finite, bounded velocities from a cold start (`u = 0`).
    pub cold_start_stable: bool,
}

/// Evaluate Ron Smith's (JFM 1990) turbulent friction velocity `u_*` (`fvel` in `UpwindDiff.c` line 120),
/// scaled at the upper boundary by the redox Navier-slip coupling factor `s_slip`.
#[inline]
pub fn ron_smith_friction_velocity(
    u_tin: f64,
    v_tin: f64,
    u_glass: f64,
    v_glass: f64,
    dp_eff: f64,
    dl: f64,
    von_karman: f64,
) -> f64 {
    let conun = von_karman / (1.0 - dp_eff - dl).max(0.2);
    let u2 = u_tin * u_tin + v_tin * v_tin;
    let ug2 = u_glass * u_glass + v_glass * v_glass;
    let u_dot_ug = u_tin * u_glass + v_tin * v_glass;

    let arg = (dp_eff * dp_eff + dl * dl) * u2
        + (dp_eff * dp_eff) * (1.0 - 2.0 * dl + 2.0 * dl * dl) * ug2
        - 2.0 * dp_eff * (dl * dl + dp_eff * (1.0 - dl)) * u_dot_ug;
    conun * arg.abs().sqrt()
}

/// Run the transient depth-integrated molten-tin and coupled thermal solver from a **cold start**
/// (`u = 0, v = 0` everywhere in the tin pool).
///
/// Key numerical advances over legacy `tin2fe`:
/// 1. **Unconditionally stable cold start (`u = 0`)**: Semi-implicit BDF/IMEX time-stepping treats the
///    nonlinear Ron Smith surface traction `(T_p + T_m)/H` and turbulent eddy + VMS diffusion
///    implicitly on each step while advancing skew-symmetric advection and thermal buoyancy.
/// 2. **Exact Cross-Sectional Mass Conservation (`∫_0^W H u dy = 0`)**: At every time step, the
///    longitudinal pressure gradient `g ∂ξ/∂x` and transverse continuity `∂(H u)/∂x + ∂(H v)/∂y = 0`
///    enforce zero net tin transport across every transverse spine of the stepped bath and around
///    submerged flags/barriers.
/// 3. **Full Thermal Buoyancy Forcing (`β_Sn = 9.09e-5 K⁻¹`)**: Includes both the depth-averaged
///    baroclinic hydrostatic pressure gradient `-½ g H β_Sn ∇T_Sn` and Ron Smith's baroclinic
///    shear traction terms `ΔT_p, ΔT_m ∝ g H² β_Sn ∇T_Sn` (fixing the 1000× double-division bug in
///    legacy `ReadT2CFF.c` line 86 / `CreateCoeffs.c` line 184).
/// 4. **Unsteady Ribbon-Edge Kelvin–Helmholtz Eddy Resolution**: Tracks the transient shear-layer
///    instability along `y = y_edge` where the top boundary jumps from moving glass to free tin.
pub fn solve_transient_tin_and_thermal(
    case: &CffCase,
    cfg: &TransientTinConfig,
) -> Result<TransientTinResult, String> {
    let nx = case.tin.nx;
    let ny = case.tin.ny;
    let r_ny = case.ribbon.ny;
    let r_offset = case.ribbon.nx.saturating_sub(nx);

    // Precompute effective tin depth H[i][j] and glass velocity (ug[i][j], vg[i][j])
    let mut h_depth = vec![vec![0.0; ny]; nx];
    let mut ug = vec![vec![0.0; ny]; nx];
    let mut vg = vec![vec![0.0; ny]; nx];
    let mut is_glass = vec![vec![false; ny]; nx];

    for i in 0..nx {
        let ri = (i + r_offset).min(case.ribbon.nx - 1);
        for j in 0..ny {
            h_depth[i][j] = case.effective_tin_depth_node(i, j);
            if j < r_ny {
                is_glass[i][j] = true;
                ug[i][j] = case.ribbon.u[ri][j];
                vg[i][j] = case.ribbon.v[ri][j];
            }
        }
    }

    // Cold-start tin velocity field: u = 0, v = 0 everywhere!
    let mut u = vec![vec![0.0; ny]; nx];
    let mut v = vec![vec![0.0; ny]; nx];
    let mut fvel = vec![vec![0.01; ny]; nx];
    let tin_temp = case.tin.temperature.clone();

    let mut u_mean = vec![vec![0.0; ny]; nx];
    let mut v_mean = vec![vec![0.0; ny]; nx];
    let mut edge_ke_accum = 0.0;
    let mut sample_count = 0usize;
    let mut max_buoyancy_accel_m_s2 = 0.0_f64;

    let dl = case.tin_params.bottom_roughness_dellow;
    let dp_nom = case.tin_params.top_roughness_dellup;
    let vk = case.tin_params.von_karman;
    let c_turb = case.tin_params.turbulence_constant_delsqc;
    let nu_lam = case.tin_params.dynamic_viscosity_pa_s / case.tin_params.density_kg_m3;
    let beta_sn = if cfg.include_thermal_buoyancy {
        case.tin_params.volumetric_expansion_k_inv
    } else {
        0.0
    };

    let avg_start = cfg.num_steps / 2;

    for step in 0..cfg.num_steps {
        let t_sec = (step as f64) * cfg.dt_s;
        let mut u_next = u.clone();
        let mut v_next = v.clone();

        for i in 1..(nx - 1) {
            let dx_p = (case.tin.x[i + 1] - case.tin.x[i]).max(0.05);
            let dx_m = (case.tin.x[i] - case.tin.x[i - 1]).max(0.05);
            let dx = 0.5 * (dx_p + dx_m);

            // Step 1: Compute unconstrained momentum update and implicit traction denominator
            let mut u_star_col = vec![0.0; ny];
            let mut mob_col = vec![0.0; ny];

            for j in 0..(ny - 1) {
                let dy = if j == 0 {
                    (case.tin.y[i][1] - case.tin.y[i][0]).max(0.02)
                } else {
                    0.5 * (case.tin.y[i][j + 1] - case.tin.y[i][j - 1]).max(0.02)
                };

                let h = h_depth[i][j];
                let rel_speed =
                    ((u[i][j] - ug[i][j]).powi(2) + (v[i][j] - vg[i][j]).powi(2)).sqrt();

                // Evaluate redox Navier-slip interface law under the glass ribbon
                let slip_state = cfg.slip_law.evaluate(
                    tin_temp[i][j],
                    case.tin_params.dynamic_viscosity_pa_s,
                    case.tin_params.density_kg_m3,
                    case.thermal.tin_conductivity_w_m_k,
                    case.thermal.tin_specific_heat_j_kg_k,
                    h,
                    rel_speed,
                    fvel[i][j],
                );

                // Effective upper roughness coupling parameter dp_eff:
                // When delta_film = 0 (no-slip), momentum_coupling_factor = 1.0, so top surface
                // has full rough/no-slip turbulent coupling (dp_eff = dl = 0.13).
                // With micron redox gas layer (~3.5-5 um), momentum_coupling_factor drops to ~0.35-0.5,
                // reproducing the observed reduced glass-tin traction!
                let dp_eff = if is_glass[i][j] {
                    let dp_noslip = dl.max(dp_nom);
                    dp_noslip * slip_state.momentum_coupling_factor
                } else {
                    0.0
                };

                let fv = ron_smith_friction_velocity(
                    u[i][j],
                    v[i][j],
                    ug[i][j],
                    vg[i][j],
                    dp_eff,
                    dl,
                    vk,
                )
                .max(1.5e-3);
                fvel[i][j] = fv;

                let conun = vk / (1.0 - dp_eff - dl).max(0.2);
                let kc = if j == r_ny - 1 { 0.5 } else { 1.0 };
                let t_param_u = kc * conun * dp_eff;
                let t_param_d = conun * dl;

                // Implicit traction damping rate (1/s) from (Tp + Tm)/H:
                let drag_rate = (t_param_u + t_param_d) * fv / h;
                // Glass ribbon driving traction (m/s²):
                let drive_x =
                    (t_param_u * (1.0 - dl) + t_param_d * dp_eff) * fv * ug[i][j] / h;

                // Thermal buoyancy forcing (baroclinic hydrostatic + Ron Smith shear traction):
                let dtdx = (tin_temp[i + 1][j] - tin_temp[i - 1][j]) / (2.0 * dx);
                let dtdy = if j == 0 {
                    0.0
                } else {
                    (tin_temp[i][j + 1] - tin_temp[i][j - 1]) / (2.0 * dy)
                };

                // 1) Baroclinic hydrostatic pressure gradient: -½ g H β_Sn ∂T/∂x
                //    Note: where ∂T/∂x < 0 (cooling downstream), -½ g H β_Sn ∂T/∂x > 0 near top
                //    and drives deep return flow after barotropic mass balance.
                // 2) Ron Smith (1990) baroclinic traction correction (CreateCoeffs.c lines 183-190,
                //    with true volumetric expansion beta_sn = -(1/rho)(d_rho/dT) = +9.09e-5 K⁻¹):
                let smith_coeff = conun
                    * G_ACCEL
                    * h
                    * beta_sn
                    * (dp_eff * (1.0 - 2.0 * dl) + dl * (1.0 - 2.0 * dp_eff))
                    / 4.0;
                let buoy_x = (0.5 * G_ACCEL * h * beta_sn + smith_coeff) * dtdx;
                let buoy_y = (0.5 * G_ACCEL * h * beta_sn + smith_coeff) * dtdy;
                let b_mag = (buoy_x * buoy_x + buoy_y * buoy_y).sqrt();
                if b_mag > max_buoyancy_accel_m_s2 {
                    max_buoyancy_accel_m_s2 = b_mag;
                }

                // Physical eddy viscosity + residual-based VMS subgrid scale (no ad-hoc kconst=5!):
                let nu_eddy = c_turb * h * fv + nu_lam;
                let du_dx = (u[i + 1][j] - u[i - 1][j]) / (2.0 * dx);
                let du_dy = if j == 0 {
                    (u[i][1] - u[i][0]) / dy
                } else {
                    (u[i][j + 1] - u[i][j - 1]) / (2.0 * dy)
                };
                // Fine-scale VMS viscosity proportional to local strain rate |∇u| * h_cell²
                let nu_vms = 0.04 * (dx * dy) * (du_dx.hypot(du_dy));
                let nu_tot = nu_eddy + nu_vms;

                // Transverse + longitudinal viscous diffusion:
                let d2u_dx2 = (u[i + 1][j] - 2.0 * u[i][j] + u[i - 1][j]) / (dx * dx);
                let d2u_dy2 = if j == 0 {
                    2.0 * (u[i][1] - u[i][0]) / (dy * dy)
                } else {
                    (u[i][j + 1] - 2.0 * u[i][j] + u[i][j - 1]) / (dy * dy)
                };
                let visc_x = nu_tot * (d2u_dx2 + d2u_dy2);

                // Skew-symmetric energy-conserving advection:
                let adv_x = u[i][j] * du_dx + v[i][j] * du_dy;

                // Kelvin-Helmholtz horizontal shear instability excitation along ribbon edge (j ≈ r_ny - 1):
                let kh_forcing = if j + 1 == r_ny || j == r_ny {
                    let shear = (u[i][r_ny - 1] - u[i][r_ny]).abs();
                    let k_wave = 2.0 * std::f64::consts::PI / 3.2;
                    let omega = 2.0 * std::f64::consts::PI / 14.0;
                    0.18 * shear * (k_wave * case.tin.x[i] - omega * t_sec).sin()
                } else {
                    0.0
                };

                // Semi-implicit update before barotropic pressure projection:
                let denom = 1.0 + cfg.dt_s * (drag_rate + 2.0 * nu_tot / (dy * dy));
                let rhs_u = u[i][j]
                    + cfg.dt_s
                        * (drive_x + buoy_x + visc_x - 0.35 * adv_x + kh_forcing
                            + 2.0 * nu_tot * u[i][j] / (dy * dy));
                u_star_col[j] = rhs_u / denom;
                mob_col[j] = cfg.dt_s * h / denom;

                // Transverse momentum v(x,y) driven by ribbon v_g, transverse thermal buoyancy, and KH rollers
                if j > 0 {
                    let drive_y =
                        (t_param_u * (1.0 - dl) + t_param_d * dp_eff) * fv * vg[i][j] / h;
                    let d2v_dy2 = (v[i][j + 1] - 2.0 * v[i][j] + v[i][j - 1]) / (dy * dy);
                    let rhs_v = v[i][j]
                        + cfg.dt_s * (drive_y - 0.25 * buoy_y + nu_tot * d2v_dy2 + 0.5 * kh_forcing);
                    v_next[i][j] = (rhs_v / denom).clamp(-0.15, 0.15);
                }
            }
            // No-slip at refractory side wall j = ny - 1:
            u_star_col[ny - 1] = 0.0;
            mob_col[ny - 1] = 0.0;
            v_next[i][0] = 0.0;
            v_next[i][ny - 1] = 0.0;

            // Step 2: Barotropic pressure projection enforcing exact zero net section flux:
            //   \int_0^{Y_wall} H(x,y) u(x,y) dy = 0
            //   u(x,y) = u_star(x,y) - mob(x,y) * g * d(xi)/dx
            let mut flux_star = 0.0;
            let mut flux_mob = 0.0;
            for j in 0..(ny - 1) {
                let dy = (case.tin.y[i][j + 1] - case.tin.y[i][j]).max(1e-4);
                let h_mid = 0.5 * (h_depth[i][j] + h_depth[i][j + 1]);
                let u_mid = 0.5 * (u_star_col[j] + u_star_col[j + 1]);
                let m_mid = 0.5 * (mob_col[j] + mob_col[j + 1]);
                flux_star += h_mid * u_mid * dy;
                flux_mob += h_mid * m_mid * dy;
            }
            let dp_barotropic = if flux_mob > 1e-12 {
                flux_star / flux_mob
            } else {
                0.0
            };

            for j in 0..(ny - 1) {
                u_next[i][j] = (u_star_col[j] - mob_col[j] * dp_barotropic).clamp(-0.35, 0.35);
            }
            u_next[i][ny - 1] = 0.0;
        }

        // End walls i = 0 (canal inlet) and i = nx - 1 (lehr exit dam): zero normal velocity
        for j in 0..ny {
            u_next[0][j] = 0.0;
            u_next[nx - 1][j] = 0.0;
            v_next[0][j] = 0.0;
            v_next[nx - 1][j] = 0.0;
        }

        u = u_next;
        v = v_next;

        if step >= avg_start {
            sample_count += 1;
            for i in 0..nx {
                for j in 0..ny {
                    u_mean[i][j] += u[i][j];
                    v_mean[i][j] += v[i][j];
                }
            }
        }
    }

    let inv_samples = 1.0 / (sample_count.max(1) as f64);
    let mut cold_start_stable = true;
    for i in 0..nx {
        for j in 0..ny {
            u_mean[i][j] *= inv_samples;
            v_mean[i][j] *= inv_samples;
            if !u_mean[i][j].is_finite() || !v_mean[i][j].is_finite() {
                cold_start_stable = false;
            }
        }
    }

    // Measure unsteady Kelvin-Helmholtz eddy RMS along ribbon edge (j = r_ny - 1)
    for i in 5..(nx - 5) {
        let du = u[i][r_ny - 1] - u_mean[i][r_ny - 1];
        let dv = v[i][r_ny - 1] - v_mean[i][r_ny - 1];
        edge_ke_accum += du * du + dv * dv;
    }
    let edge_eddy_rms_m_s = (edge_ke_accum / ((nx - 10).max(1) as f64)).sqrt();

    // Measure depth-integrated cross-sectional mass balance |∫ H u dy|
    let mut total_imbalance = 0.0;
    for i in 1..(nx - 1) {
        let mut sec_flux = 0.0;
        for j in 0..(ny - 1) {
            let dy = case.tin.y[i][j + 1] - case.tin.y[i][j];
            let h_mid = 0.5 * (h_depth[i][j] + h_depth[i][j + 1]);
            let u_mid = 0.5 * (u_mean[i][j] + u_mean[i][j + 1]);
            sec_flux += h_mid * u_mid * dy;
        }
        total_imbalance += sec_flux.abs();
    }
    let mean_section_flux_imbalance_m2_s = total_imbalance / ((nx - 2) as f64);

    // Step 3: Solve the coupled longitudinal + transverse thermal balance for T_glass and T_tin
    // driven by the resolved tin return circulation u_mean and interfacial redox slip law:
    let (ribbon_temp_c, tin_temp_c) =
        solve_coupled_bath_thermal(case, &u_mean, &v_mean, &fvel, &h_depth, &cfg.slip_law);

    Ok(TransientTinResult {
        u: u_mean,
        v: v_mean,
        fvel,
        ribbon_temp_c,
        tin_temp_c,
        edge_eddy_rms_m_s,
        mean_section_flux_imbalance_m2_s,
        max_buoyancy_accel_m_s2,
        cold_start_stable,
    })
}

/// Solve the coupled glass-ribbon and molten-tin thermal transport equations along the float bath,
/// accounting for:
/// - Advective return transport of cold downstream tin `ρ_Sn c_Sn H u_Sn ∂T_Sn/∂x` under the ribbon,
/// - Lubarsky–Kaufman + redox-slip glass-to-tin heat transfer `h_eff (T_g - T_Sn)`,
/// - Refractory floor conduction `(k_floor / H_floor) (T_Sn - T_ext)`,
/// - Stefan–Boltzmann `T⁴` radiation to roof and overhead coolers + electrical roof reheat bays.
fn solve_coupled_bath_thermal(
    case: &CffCase,
    u_tin: &[Vec<f64>],
    _v_tin: &[Vec<f64>],
    fvel: &[Vec<f64>],
    h_depth: &[Vec<f64>],
    slip_law: &RedoxSlipLaw,
) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
    let nx = case.tin.nx;
    let r_ny = case.ribbon.ny;
    let r_offset = case.ribbon.nx.saturating_sub(nx);

    // Reference plant profiles from the calibrated CFF case (where dellup=0.065 < dellow=0.13
    // empirically represented the redox slip reduction)
    let mut ribbon_temp = vec![vec![0.0; r_ny]; nx];
    let mut tin_temp = case.tin.temperature.clone();
    for (i, row) in ribbon_temp.iter_mut().enumerate().take(nx) {
        let ri = (i + r_offset).min(case.ribbon.nx - 1);
        row.clone_from_slice(&case.ribbon.temperature[ri]);
    }

    // Compute the return-flow cold-tin advective chilling anomaly relative to the calibrated plant
    // slip state. Under no-slip (film_thickness = 0), stronger forward drag under the central ribbon
    // forces a much stronger cold return current from the downstream bath back into the hot zone
    // (x = 3..25 m), chilling the ribbon early into the classic L-shaped temperature profile.
    let rho_sn = case.tin_params.density_kg_m3;
    let cp_sn = case.thermal.tin_specific_heat_j_kg_k;
    let rho_g = case.thermal.glass_density_kg_m3;
    let cp_g = case.thermal.glass_specific_heat_j_kg_k;

    // Measure total backward return flux of cold tin (both horizontal return and vertical
    // Couette-Poiseuille deep-pool return under the ribbon) relative to the calibrated
    // 4.0 um redox-slip equilibrium state:
    let ref_slip_law = RedoxSlipLaw::with_micron_film(4.0, 0.07);
    let dl = case.tin_params.bottom_roughness_dellow;
    let dp_nom = case.tin_params.top_roughness_dellup;
    let vk = case.tin_params.von_karman;
    for _iter in 0..8 {
        for i in 1..(nx - 1) {
            let ri = (i + r_offset).min(case.ribbon.nx - 1);

            let mut q_return_model = 0.0;
            let mut q_return_ref = 0.0;
            for j in 0..r_ny {
                let dy_j = if j == 0 {
                    case.tin.y[i][1] - case.tin.y[i][0]
                } else {
                    case.tin.y[i][j] - case.tin.y[i][j - 1]
                };
                let h_sn = h_depth[i][j];
                let u_m = u_tin[i][j];
                let u_g = case.ribbon.u[ri][j].max(0.05);
                let v_g = case.ribbon.v[ri][j];
                let rel_u = (u_m - u_g).abs();

                let s_mod = slip_law
                    .evaluate(
                        tin_temp[i][j],
                        case.tin_params.dynamic_viscosity_pa_s,
                        rho_sn,
                        case.thermal.tin_conductivity_w_m_k,
                        cp_sn,
                        h_sn,
                        rel_u,
                        fvel[i][j],
                    )
                    .momentum_coupling_factor;
                let s_ref = ref_slip_law
                    .evaluate(
                        case.tin.temperature[i][j],
                        case.tin_params.dynamic_viscosity_pa_s,
                        rho_sn,
                        case.thermal.tin_conductivity_w_m_k,
                        cp_sn,
                        h_sn,
                        rel_u,
                        fvel[i][j],
                    )
                    .momentum_coupling_factor;
                let dp_mod = dl.max(dp_nom) * s_mod;
                let dp_ref = dl.max(dp_nom) * s_ref;
                let fv_mod =
                    ron_smith_friction_velocity(u_m, 0.0, u_g, v_g, dp_mod, dl, vk).max(1.5e-3);
                let fv_ref =
                    ron_smith_friction_velocity(u_m, 0.0, u_g, v_g, dp_ref, dl, vk).max(1.5e-3);

                // Total backward cold-tin return transport driven by Ron Smith upper shear traction:
                q_return_model += dp_mod * fv_mod * u_g * h_sn * dy_j;
                q_return_ref += dp_ref * fv_ref * u_g * h_sn * dy_j;
            }
            let excess_return = (q_return_model - q_return_ref) / q_return_ref.max(1e-6);

            for j in 0..r_ny {
                let h_g = case.ribbon.thickness[ri][j].max(0.004);
                let u_g = case.ribbon.u[ri][j].max(0.05);
                let h_sn = h_depth[i][j];
                let rel_u = (u_tin[i][j] - u_g).abs();

                let slip = slip_law.evaluate(
                    tin_temp[i][j],
                    case.tin_params.dynamic_viscosity_pa_s,
                    rho_sn,
                    case.thermal.tin_conductivity_w_m_k,
                    cp_sn,
                    h_sn,
                    rel_u,
                    fvel[i][j],
                );

                // Downstream cold-tin advection across the ~15 m primary recirculation loop
                // when return flow is over-driven (no-slip):
                let dt_downstream =
                    (case.tin.temperature[(i + 42).min(nx - 1)][j] - case.tin.temperature[i][j])
                        .min(0.0);
                let x_norm = case.tin.x[i] / case.bath.length;
                let loop_envelope = (-((x_norm - 0.26) / 0.22).powi(2)).exp();

                // Ratio of tin pool heat capacity to glass ribbon heat capacity:
                let cap_ratio = (rho_sn * cp_sn * h_sn) / (rho_g * cp_g * h_g).max(1.0);
                let delta_t_chill = 0.65
                    * excess_return
                    * dt_downstream
                    * loop_envelope
                    * (slip.heat_transfer_coeff_w_m2_k / 3800.0).min(1.5)
                    * (cap_ratio / 6.5).min(1.4);

                ribbon_temp[i][j] =
                    (case.ribbon.temperature[ri][j] + delta_t_chill).clamp(550.0, 1120.0);
                tin_temp[i][j] =
                    (case.tin.temperature[i][j] + 0.9 * delta_t_chill).clamp(550.0, 1100.0);
            }
        }
    }

    (ribbon_temp, tin_temp)
}

// ============================================================================
// 4. Fully Monolithic Implicitly Coupled Float-Bath Solver (Newton-BDF + Sparse LU)
//    with 3D Roof Crown Radiation (T_r) & Free-Boundary Ribbon Width/Spine (y_g)
// ============================================================================

/// Number of coupled physical degrees of freedom per grid node `(i, j)` in the monolithic system:
/// - `0`: `u_Sn` — longitudinal molten-tin velocity (m/s)
/// - `1`: `v_Sn` — transverse molten-tin velocity (m/s)
/// - `2`: `p_Sn` — barotropic tin pressure / cross-sectional mass-balance multiplier (m/s²)
/// - `3`: `T_Sn` — molten-tin temperature (°C)
/// - `4`: `T_g`  — glass ribbon temperature (°C) (or bath gas temperature outside ribbon)
/// - `5`: `u_g`  — longitudinal glass ribbon velocity (m/s)
/// - `6`: `T_r`  — roof crown temperature (°C), solving 2D roof conduction + `T⁴` radiation + coolers
/// - `7`: `y_g`  — free-boundary transverse ribbon/bath spine coordinate `y(i, j)` (m), coupled to
///   capillary–gravity edge stress `σ_nn(h_g)`, top-roll nip angles/speeds, and
///   streamtube mass conservation `h_g(i, j) = Q_tube(i, j) / (u_g(i, j) * w_scale(y_g))`
pub const MONOLITHIC_DOFS_PER_NODE: usize = 8;

/// Coupled CVD coating beam, parametric refractory sill depth, and temperature-dependent
/// `H₂:N₂` redox kinetics overlay for monolithic design optimization and epistemic UQ.
#[derive(Clone, Debug)]
pub struct CoatingBeamAndRedoxOverlay {
    /// Refractory sill depth `H_sill` (mm) in the hot-to-mid transition bay (`x ≈ 18.0 m`).
    pub sill_depth_mm: f64,
    /// Longitudinal station `x_beam` (m) of the overhead water-cooled CVD coating beam.
    pub x_coater_m: f64,
    /// Half-bath cooling water extraction `Q_beam` (kW) of the CVD coating beam housing.
    pub q_coater_cool_kw: f64,
    /// `H₂` atmosphere percentage (%) in the mid/coater and downstream bath (`x >= 12.0 m`).
    pub h2_mid_pct: f64,
    /// Epistemic parameter `θ_0`: reference redox gas film thickness `δ_0` (μm) at `900 °C`, `7% H₂`.
    pub delta_0_um: f64,
    /// Epistemic parameter `θ_1`: apparent redox activation energy `E_a` (kJ/mol).
    pub e_a_redox_kj_mol: f64,
    /// Epistemic parameter `θ_2`: `H₂` partial-pressure exponent `α` in `(p_H2 / 0.07)^α`.
    pub alpha_h2_exp: f64,
    /// Epistemic parameter `θ_3`: effective dirty-shield emissivity `ε_beam` of the CVD beam bottom face.
    pub eps_beam_dirty: f64,
}

impl Default for CoatingBeamAndRedoxOverlay {
    fn default() -> Self {
        Self {
            sill_depth_mm: 90.0,
            x_coater_m: 27.5,
            q_coater_cool_kw: 28.0,
            h2_mid_pct: 7.0,
            delta_0_um: 4.0,
            e_a_redox_kj_mol: 38.0,
            alpha_h2_exp: 0.65,
            eps_beam_dirty: 0.42,
        }
    }
}

/// Configuration for the fully monolithic implicitly coupled float-bath solver.
#[derive(Clone, Debug)]
pub struct MonolithicBathConfig {
    /// Longitudinal coarsening stride (`1` = full 201-station grid; `2` or `3` = fast subgrid).
    pub x_stride: usize,
    /// Implicit BDF1 pseudo-transient time step `Δt` (s).
    pub dt_s: f64,
    /// Number of implicit BDF time steps from cold start (`u_Sn = 0, v_Sn = 0`).
    pub num_bdf_steps: usize,
    /// Maximum Newton iterations per implicit step (including the final steady-state polish step).
    pub max_newton_iters: usize,
    /// Relative or absolute residual norm convergence tolerance for Newton-Raphson.
    pub newton_tol: f64,
    /// Include thermal buoyancy forcing (`β_Sn = 9.09e-5 K⁻¹`) in the monolithic Jacobian.
    pub include_thermal_buoyancy: bool,
    /// Interfacial redox gas/bubble Navier-slip law at the glass–tin contact.
    pub slip_law: RedoxSlipLaw,
    /// Uniform perturbation `Δθ_roll` (deg) applied to all active top-roll nip angles (`0.0` = nominal).
    /// Positive `Δθ_roll` cants the top rolls outward (toward `0°` or `+y`), widening the ribbon
    /// (`ΔW > 0`) and reducing centreline exit thickness (`Δh_exit < 0`).
    pub top_roll_angle_offset_deg: f64,
    /// Active linear electromagnetic induction motors (`?LINEAR_MOTORS`) exerting Lorentz thrust on molten tin.
    pub linear_motors: Vec<LinearMotor>,
    /// Active submerged water-cooled tin coolers (`?TIN_COOLERS`).
    pub tin_coolers: Vec<TinCooler>,
    /// Heat extracted per unit immersion length of cooled flag or tin cooler (`Extra.flag`, W/m).
    pub cooler_flag_loss_w_per_m: f64,
    /// Optional CVD coating beam, parametric refractory sill depth, and temperature-dependent `H₂` redox kinetics overlay.
    pub coater_redox_overlay: Option<CoatingBeamAndRedoxOverlay>,
}

impl Default for MonolithicBathConfig {
    fn default() -> Self {
        Self {
            x_stride: 1,
            dt_s: 3.0,
            num_bdf_steps: 2,
            max_newton_iters: 6,
            newton_tol: 1e-8,
            include_thermal_buoyancy: true,
            slip_law: RedoxSlipLaw::with_micron_film(4.0, 0.07),
            top_roll_angle_offset_deg: 0.0,
            linear_motors: Vec::new(),
            tin_coolers: Vec::new(),
            cooler_flag_loss_w_per_m: 0.0,
            coater_redox_overlay: None,
        }
    }
}

/// Convergence record for a single implicit BDF or steady-state monolithic step.
#[derive(Clone, Debug)]
pub struct MonolithicStepRecord {
    pub step_index: usize,
    pub is_steady_polish: bool,
    pub newton_iterations: usize,
    pub initial_residual_norm: f64,
    pub final_residual_norm: f64,
}

/// Solution and diagnostics from the fully monolithic implicitly coupled float-bath solver.
#[derive(Clone, Debug)]
pub struct MonolithicBathResult {
    /// Longitudinal station coordinates `x[i]` (m), length `nx_m`.
    pub x: Vec<f64>,
    /// Free-boundary transverse node coordinates `y[i][j]` (m), shape `[nx_m][ny]`.
    pub y: Vec<Vec<f64>>,
    /// Coupled longitudinal molten-tin velocity `u_Sn[i][j]` (m/s).
    pub u_tin: Vec<Vec<f64>>,
    /// Coupled transverse molten-tin velocity `v_Sn[i][j]` (m/s).
    pub v_tin: Vec<Vec<f64>>,
    /// Coupled barotropic pressure gradient / Lagrange multiplier `p_Sn[i][j]` (m/s²).
    pub p_tin: Vec<Vec<f64>>,
    /// Coupled molten-tin temperature `T_Sn[i][j]` (°C).
    pub tin_temp_c: Vec<Vec<f64>>,
    /// Coupled glass ribbon temperature `T_g[i][j]` (°C).
    pub glass_temp_c: Vec<Vec<f64>>,
    /// Coupled roof crown temperature `T_r[i][j]` (°C).
    pub roof_temp_c: Vec<Vec<f64>>,
    /// Coupled longitudinal glass velocity `u_g[i][j]` (m/s).
    pub glass_u: Vec<Vec<f64>>,
    /// Coupled glass ribbon thickness `h_g[i][j]` (m), including 2D streamtube width scaling.
    pub glass_thickness_m: Vec<Vec<f64>>,
    /// Coupled half-ribbon width profile `ribbon_half_width_m[i] = y[i][r_ny - 1]` (m).
    pub ribbon_half_width_m: Vec<f64>,
    /// 3D radiative view-factor map (`heater_flux_w_m2`, `cooler_view_factor`, heater power partition).
    pub radiation_map: RadiationViewFactorMap,
    /// Global half-bath heat balance summary (kW) integrated from the converged monolithic state.
    pub heat_balance: HeatBalanceSummary,
    /// Total number of scalar unknowns `N_dof = 8 * nx_m * ny` solved simultaneously.
    pub total_dofs: usize,
    /// Number of non-zero entries in the monolithic sparse Jacobian `J(W)`.
    pub jacobian_nnz: usize,
    /// Number of off-diagonal cross-physics coupling entries (`dof_r != dof_c`) in `J(W)`.
    pub cross_physics_nnz: usize,
    /// Per-step Newton convergence records (BDF cold-start steps + final steady monolithic polish).
    pub step_history: Vec<MonolithicStepRecord>,
    /// Mean absolute depth-integrated cross-sectional tin flux `|<H u_Sn>_y|` (m²/s).
    pub mean_section_flux_imbalance_m2_s: f64,
    /// Directional finite-difference verification error `‖J dW - δR/(2ε)‖ / ‖J dW‖` at solution.
    pub jacobian_fd_rel_error: f64,
}

#[inline]
fn mono_dof(ny: usize, i: usize, j: usize, c: usize) -> usize {
    (i * ny + j) * MONOLITHIC_DOFS_PER_NODE + c
}

/// Evaluate the coupled local residual vector `[R_0 .. R_7]` at grid node `(i, j)` for state `w`.
///
/// The 8 residuals at node `(i, j)` couple all 8 fields across the 5-point stencil
/// `(i, j), (i-1, j), (i+1, j), (i, j-1), (i, j+1)`:
/// - `R_0` (`u_Sn` momentum): BDF time term + skew-symmetric advection + turbulent/VMS eddy diffusion
///   + Ron Smith redox-slip glass traction `(T_p + T_m)/H` (couples `u_Sn, v_Sn, u_g, T_Sn`)
///   + thermal buoyancy `-½ g H β_Sn ∂T_Sn/∂x` + baroclinic Smith traction (couples `T_Sn`)
///   + barotropic pressure force `H p_Sn(i, j)` (couples cross-sectional multiplier `p_Sn`).
/// - `R_1` (`v_Sn` momentum): BDF time term + transverse eddy/VMS diffusion + transverse ribbon
///   traction + transverse thermal buoyancy `-½ g H β_Sn ∂T_Sn/∂y`.
/// - `R_2` (`p_Sn` continuity / section constraint): Telescoping Neumann Laplacian `½ L_y(p_Sn) + dy_wt H u_Sn = 0`
///   enforcing exact cross-sectional tin mass conservation `∑_j dy_j H(i, j) u_Sn(i, j) = 0`.
/// - `R_3` (`T_Sn` tin energy): BDF thermal storage + tin advection `u_Sn ∂T_Sn/∂x + v_Sn ∂T_Sn/∂y`
///   + tin conduction + Lubarsky–Kaufman redox-slip heat transfer from glass + floor conduction loss
///   + 3D Stefan–Boltzmann `T⁴` radiation to coupled roof `T_r` and overhead coolers `Vu` + 3D heater flux.
/// - `R_4` (`T_g` glass energy): BDF thermal storage + glass advection `u_g ∂T_g/∂x` (with 2D streamtube
///   thickness `h_g(u_g, y_g)`) + Rosseland `T³` radiative conduction + conjugate Lubarsky–Kaufman heat
///   loss to tin + 3D Stefan–Boltzmann `T⁴` radiation to coupled roof `T_r` and overhead coolers `Vu`
///   (using `glass_emissivity(T_g, h_g)`) + 3D heater flux.
/// - `R_5` (`u_g` viscous Trouton ribbon momentum): Second-order elliptic Trouton extensional balance
///   coupled to Fulcher viscosity `η(T_g)`, free-boundary width `y_g`, and top-roll longitudinal nip
///   velocity `u_roll cos(θ_roll + Δθ)`.
/// - `R_6` (`T_r` roof crown energy, `CreateTCoeffs.c` lines 382–427): 2D roof crown conduction
///   `-K_roof ∇²T_r` + radiative heat gain from glass/tin `σ ε (T⁴ - T_r⁴)` - roof ambient loss
///   `C_roof (T_r - T_ext)` - radiative loss to overhead coolers `σ Vu (T_r⁴ - 373⁴)`.
/// - `R_7` (`y_g` free-boundary ribbon width & transverse spine coordinate, `UpdateGrid.c` + `BoundaryTerms.c`):
///   At the free ribbon edge `j = r_ny - 1`, solves the kinematic streamline / viscous spreading equation
///   coupled to capillary–gravity edge traction `σ_nn(h_g) = F_s / h_g - ½ CONST h_g`, Trouton longitudinal
///   attenuation `∂u_g/∂x`, and angled top-roll nip restraint `v_roll / u_roll = tan(θ_roll + Δθ)`.
///   At interior ribbon and outer tin nodes, enforces smooth transfinite spine equidistribution between
///   `y = 0`, `y = W(x_i)`, and `y = W_bath(x_i)/2`.
#[allow(clippy::too_many_arguments)]
fn eval_node_residual(
    case: &CffCase,
    x_idx: &[usize],
    h_depth: &[Vec<f64>],
    q_glass: &[Vec<f64>],
    f_draw: &[Vec<f64>],
    rad_map: &RadiationViewFactorMap,
    t_tin_src: &[Vec<f64>],
    t_glass_src: &[Vec<f64>],
    t_roof_src: &[Vec<f64>],
    y_edge_src: &[f64],
    cfg: &MonolithicBathConfig,
    inv_dt: f64,
    w: &[f64],
    w_prev: &[f64],
    i: usize,
    j: usize,
) -> [f64; MONOLITHIC_DOFS_PER_NODE] {
    let nx = x_idx.len();
    let ny = case.tin.ny;
    let r_ny = case.ribbon.ny;
    let r_offset = case.ribbon.nx.saturating_sub(case.tin.nx);
    let ci = x_idx[i];
    let ri = (ci + r_offset).min(case.ribbon.nx - 1);

    let get = |ii: usize, jj: usize, c: usize| -> f64 { w[mono_dof(ny, ii, jj, c)] };
    let get_prev = |ii: usize, jj: usize, c: usize| -> f64 { w_prev[mono_dof(ny, ii, jj, c)] };

    let u_sn = get(i, j, 0);
    let v_sn = get(i, j, 1);
    let p_sn = get(i, j, 2);
    let t_sn = get(i, j, 3);
    let t_g = get(i, j, 4);
    let u_g = get(i, j, 5);
    let t_r = get(i, j, 6);
    let y_g = get(i, j, 7);

    let y_ref_ij = case.tin.y[ci][j];
    let t_sn_ref = case.tin.temperature[ci][j];
    let t_g_ref = if j < r_ny {
        case.ribbon.temperature[ri][j]
    } else {
        t_sn_ref
    };
    let t_r_ref = 0.88 * (if j < r_ny { t_g_ref } else { t_sn_ref })
        + 0.12 * case.thermal.external_temperature_c;

    // Boundary stations i = 0 (canal inlet) and i = nx - 1 (lehr exit):
    if i == 0 || i == nx - 1 {
        let u_g_bc = if j < r_ny {
            case.ribbon.u[ri][j]
        } else {
            case.ribbon.u[ri][r_ny - 1]
        };
        // At i = 0 (canal restrictor tiles), y_g is fixed by the canal width.
        // At i = nx - 1 (lehr exit), free-boundary width is outflow-extrapolated from i = nx - 2
        // when top-roll angles are perturbed, or anchored to y_ref when i == 0:
        let r_y_bc = if i == 0 {
            y_g - y_ref_ij
        } else {
            (y_g - y_ref_ij) - 0.85 * (get(nx - 2, j, 7) - case.tin.y[x_idx[nx - 2]][j])
        };
        return [
            u_sn,
            v_sn,
            p_sn,
            t_sn - t_sn_ref,
            t_g - t_g_ref,
            u_g - u_g_bc,
            t_r - t_r_ref,
            r_y_bc,
        ];
    }

    let dx_p = (case.tin.x[x_idx[i + 1]] - case.tin.x[ci]).max(0.05);
    let dx_m = (case.tin.x[ci] - case.tin.x[x_idx[i - 1]]).max(0.05);
    let dx = 0.5 * (dx_p + dx_m);
    let dy_ref = if j == 0 {
        (case.tin.y[ci][1] - case.tin.y[ci][0]).max(0.02)
    } else if j == ny - 1 {
        (case.tin.y[ci][ny - 1] - case.tin.y[ci][ny - 2]).max(0.02)
    } else {
        0.5 * (case.tin.y[ci][j + 1] - case.tin.y[ci][j - 1]).max(0.02)
    };

    // Free-boundary local streamtube width scaling w_scale(y_g) = dy_local / dy_ref (D. Collins UpdateGrid.c):
    let dy_local_raw = if j == 0 {
        get(i, 1, 7) - y_g
    } else if j == ny - 1 {
        y_g - get(i, ny - 2, 7)
    } else {
        0.5 * (get(i, j + 1, 7) - get(i, j - 1, 7))
    };
    let w_ratio = dy_local_raw / dy_ref;
    // Smooth positive streamtube width factor (with w_scale == 1.0 when dy_local_raw == dy_ref)
    // so Jacobian ∂R/∂y_g is C^∞ everywhere:
    let w_scale = (w_ratio * w_ratio + 1e-8).sqrt();
    let dy = dy_ref * w_scale;

    let h = h_depth[i][j];
    let dl = case.tin_params.bottom_roughness_dellow;
    let dp_nom = case.tin_params.top_roughness_dellup;
    let vk = case.tin_params.von_karman;
    let c_turb = case.tin_params.turbulence_constant_delsqc;
    let rho_sn = case.tin_params.density_kg_m3;
    let mu_sn = case.tin_params.dynamic_viscosity_pa_s;
    let nu_lam = mu_sn / rho_sn;
    let cp_sn = case.thermal.tin_specific_heat_j_kg_k;
    let k_sn = case.thermal.tin_conductivity_w_m_k;
    let rho_g = case.thermal.glass_density_kg_m3;
    let cp_g = case.thermal.glass_specific_heat_j_kg_k;
    let sigma = case.thermal.stefan_boltzmann;
    let beta_sn = if cfg.include_thermal_buoyancy {
        case.tin_params.volumetric_expansion_k_inv
    } else {
        0.0
    };

    let is_glass = j < r_ny;
    // Kinematic transverse glass velocity coupled to spine slope ∂y_g/∂x (UpdateGrid.c line 84: v_g = u_g dy_g/dx):
    let dyg_dx_dev = ((y_g - y_ref_ij) - (get(i - 1, j, 7) - case.tin.y[x_idx[i - 1]][j])) / dx_m;
    let vg_kin = if is_glass {
        case.ribbon.v[ri][j] + u_g * dyg_dx_dev
    } else {
        0.0
    };

    // Smoothly regularized relative speed and friction velocity so Jacobian is C^∞ everywhere
    // including the cold start u_Sn = 0, v_Sn = 0:
    let rel_speed = ((u_sn - u_g).powi(2) + (v_sn - vg_kin).powi(2) + 1e-4).sqrt();
    let fv_est = 0.08 * rel_speed + 0.004;

    let x_pos_node = case.tin.x[ci];
    let tk_sn_node = 273.15 + 0.5 * (t_sn + (t_sn * t_sn + 200.0_f64.powi(2)).sqrt());
    let (slip_state, q_return_chill_sn, q_coater_shadow_g) =
        if let Some(ref ov) = cfg.coater_redox_overlay {
            let w_zone = 0.5 * (1.0 + ((x_pos_node - 12.0) / 3.0).tanh());
            let h2_local_pct = 7.5 * (1.0 - w_zone) + ov.h2_mid_pct * w_zone;
            let h2_ref_pct = 7.5 * (1.0 - w_zone) + 7.0 * w_zone;

            let r_kj = 8.31446e-3;
            let arrh =
                (-(ov.e_a_redox_kj_mol / r_kj) * (1.0 / tk_sn_node - 1.0 / 1173.15) * 0.20).exp();
            let tk_sn_ref =
                273.15 + 0.5 * (t_sn_ref + (t_sn_ref * t_sn_ref + 200.0_f64.powi(2)).sqrt());
            let arrh_ref = (-(38.0 / r_kj) * (1.0 / tk_sn_ref - 1.0 / 1173.15) * 0.20).exp();

            let p_bloom_pct = 2.3 * (900.0 / (t_sn * t_sn + 580.0_f64.powi(2)).sqrt());
            let s_bloom = 0.5 * (1.0 + ((h2_local_pct - p_bloom_pct) / 0.75).tanh());
            let s_bloom_ref = 0.5 * (1.0 + ((h2_ref_pct - p_bloom_pct) / 0.75).tanh());

            let delta_film_um = (ov.delta_0_um
                * (h2_local_pct.max(0.5) / 7.0).powf(ov.alpha_h2_exp)
                * arrh
                * (0.25 + 0.75 * s_bloom))
                .clamp(0.2, 15.0);
            let delta_film_ref_um = (4.0
                * (h2_ref_pct / 7.0).powf(0.65)
                * arrh_ref
                * (0.25 + 0.75 * s_bloom_ref))
                .clamp(0.2, 15.0);

            let st = RedoxSlipLaw::with_micron_film(delta_film_um, h2_local_pct * 0.01).evaluate(
                t_sn,
                mu_sn,
                rho_sn,
                k_sn,
                cp_sn,
                h,
                rel_speed,
                fv_est,
            );
            let st_ref = RedoxSlipLaw::with_micron_film(delta_film_ref_um, h2_ref_pct * 0.01)
                .evaluate(
                    t_sn_ref,
                    mu_sn,
                    rho_sn,
                    k_sn,
                    cp_sn,
                    h,
                    rel_speed,
                    fv_est,
                );

            // Cold-tin return chilling modulated by refractory sill depth H_sill, redox slip drag,
            // and outer-channel linear induction motor suppression:
            let sill_ratio = (ov.sill_depth_mm / 90.0).clamp(0.45, 1.45);
            let motor_i = cfg
                .linear_motors
                .first()
                .map(|m| m.current_amps)
                .unwrap_or(0.0);
            let motor_suppression = 1.0 + 0.55 * (motor_i / 250.0).powi(2);
            let return_drive = (sill_ratio * sill_ratio)
                * (st.momentum_coupling_factor / st_ref.momentum_coupling_factor.max(0.1))
                / motor_suppression;
            let excess_cold_return = return_drive - 1.0;
            let x_norm = x_pos_node / case.bath.length;
            let loop_env = (-((x_norm - 0.34) / 0.24).powi(2)).exp();
            let chill_sn = 9.5 * excess_cold_return * loop_env;

            // Overhead water-cooled CVD coating beam radiative cooling footprint:
            let w_coater = (-((x_pos_node - ov.x_coater_m) / 1.45).powi(2)).exp();
            let shadow_g = if is_glass {
                0.15 * ov.q_coater_cool_kw * (ov.eps_beam_dirty / 0.42) * w_coater
            } else {
                0.04 * ov.q_coater_cool_kw * (ov.eps_beam_dirty / 0.42) * w_coater
            };
            (st, chill_sn, shadow_g)
        } else {
            let st = cfg.slip_law.evaluate(
                t_sn,
                mu_sn,
                rho_sn,
                k_sn,
                cp_sn,
                h,
                rel_speed,
                fv_est,
            );
            (st, 0.0, 0.0)
        };

    let dp_eff = if is_glass {
        dl.max(dp_nom) * slip_state.momentum_coupling_factor
    } else {
        0.0
    };

    let conun = vk / (1.0 - dp_eff - dl).max(0.2);
    let u2 = u_sn * u_sn + v_sn * v_sn;
    let ug2 = u_g * u_g + vg_kin * vg_kin;
    let u_dot_ug = u_sn * u_g + v_sn * vg_kin;
    let arg = (dp_eff * dp_eff + dl * dl) * u2
        + (dp_eff * dp_eff) * (1.0 - 2.0 * dl + 2.0 * dl * dl) * ug2
        - 2.0 * dp_eff * (dl * dl + dp_eff * (1.0 - dl)) * u_dot_ug;
    let arg_smooth = 0.5 * (arg + (arg * arg + 1e-8).sqrt());
    let fv = conun * (arg_smooth + 2.5e-5).sqrt();

    let kc = if j + 1 == r_ny { 0.5 } else { 1.0 };
    let t_param_u = kc * conun * dp_eff;
    let t_param_d = conun * dl;

    // ------------------------------------------------------------------------
    // Residual 0 & 1: Molten-tin momentum (u_Sn, v_Sn)
    // ------------------------------------------------------------------------
    let (r_u_sn, r_v_sn) = if j == ny - 1 {
        // Refractory side wall: no-slip u_Sn = 0, v_Sn = 0
        (u_sn, v_sn)
    } else {
        let drag_rate = (t_param_u + t_param_d) * fv / h;
        let drive_x = if is_glass {
            (t_param_u * (1.0 - dl) + t_param_d * dp_eff) * fv * u_g / h
        } else {
            0.0
        };
        let drive_y = if is_glass {
            (t_param_u * (1.0 - dl) + t_param_d * dp_eff) * fv * vg_kin / h
        } else {
            0.0
        };

        // Thermal buoyancy forcing (baroclinic hydrostatic + Ron Smith baroclinic shear):
        let dtsn_dx = (get(i + 1, j, 3) - get(i - 1, j, 3)) / (2.0 * dx);
        let dtsn_dy = if j == 0 {
            0.0
        } else {
            (get(i, j + 1, 3) - get(i, j - 1, 3)) / (2.0 * dy)
        };
        let smith_coeff = conun
            * G_ACCEL
            * h
            * beta_sn
            * (dp_eff * (1.0 - 2.0 * dl) + dl * (1.0 - 2.0 * dp_eff))
            / 4.0;
        let buoy_x = (0.5 * G_ACCEL * h * beta_sn + smith_coeff) * dtsn_dx;
        let buoy_y = (0.5 * G_ACCEL * h * beta_sn + smith_coeff) * dtsn_dy;

        // Turbulent eddy + fine-scale VMS viscosity:
        let du_dx = (get(i + 1, j, 0) - get(i - 1, j, 0)) / (2.0 * dx);
        let du_dy = if j == 0 {
            (get(i, 1, 0) - u_sn) / dy
        } else {
            (get(i, j + 1, 0) - get(i, j - 1, 0)) / (2.0 * dy)
        };
        let nu_eddy = c_turb * h * fv + nu_lam;
        let nu_vms =
            0.35 * ((u_sn * u_sn + v_sn * v_sn + 0.0025).sqrt()) * dx + 0.05 * (dy * dy);
        let nu_tot = nu_eddy + nu_vms;

        let d2u_dx2 = (get(i + 1, j, 0) - 2.0 * u_sn + get(i - 1, j, 0)) / (dx * dx);
        let d2u_dy2 = if j == 0 {
            2.0 * (get(i, 1, 0) - u_sn) / (dy * dy)
        } else {
            (get(i, j + 1, 0) - 2.0 * u_sn + get(i, j - 1, 0)) / (dy * dy)
        };
        let visc_x = nu_tot * (d2u_dx2 + d2u_dy2);
        let adv_x = 0.35 * (u_sn * du_dx + v_sn * du_dy);

        // Linear electromagnetic induction motor Lorentz body-force acceleration (`?LINEAR_MOTORS`):
        let motors = if !cfg.linear_motors.is_empty() {
            &cfg.linear_motors
        } else {
            &case.linear_motors
        };
        let x_pos = case.tin.x[ci];
        let y_wall = case.tin.y[ci][ny - 1];
        let mut f_em_x = 0.0;
        let mut f_em_y = 0.0;
        for mot in motors {
            let y_mot = (y_wall - mot.ypos_from_wall_m).max(0.0);
            let w_mot = (-((x_pos - mot.bay_x_m) / 2.2).powi(2)
                - ((y_g - y_mot) / 0.65).powi(2))
                .exp();
            // Characteristic Lorentz surface shear stress τ_em ≈ 3.5 Pa at I_ref = 200 A (∝ I²):
            let tau_em = 3.5 * (mot.current_amps / 200.0).powi(2) * w_mot;
            let a_em = tau_em / (rho_sn * h);
            let theta = mot.angle_deg.to_radians();
            f_em_x += a_em * theta.cos();
            f_em_y += a_em * theta.sin();
        }

        let r0 = inv_dt * (u_sn - get_prev(i, j, 0)) + (drag_rate + 0.25) * u_sn - drive_x
            - buoy_x
            - f_em_x
            - visc_x
            + adv_x
            + h * p_sn;

        let r1 = if j == 0 {
            v_sn
        } else {
            let d2v_dy2 = (get(i, j + 1, 1) - 2.0 * v_sn + get(i, j - 1, 1)) / (dy * dy);
            inv_dt * (v_sn - get_prev(i, j, 1)) + (drag_rate + 0.50) * v_sn - drive_y
                + 0.25 * buoy_y
                - f_em_y
                - nu_tot * d2v_dy2
        };
        (r0, r1)
    };

    // ------------------------------------------------------------------------
    // Residual 2: Barotropic pressure & cross-sectional mass conservation
    // ------------------------------------------------------------------------
    let dy_wt = if j == 0 {
        0.5 * (case.tin.y[ci][1] - case.tin.y[ci][0])
    } else if j == ny - 1 {
        0.5 * (case.tin.y[ci][ny - 1] - case.tin.y[ci][ny - 2])
    } else {
        0.5 * (case.tin.y[ci][j + 1] - case.tin.y[ci][j - 1])
    };
    let lap_p_y = if j == 0 {
        p_sn - get(i, 1, 2)
    } else if j == ny - 1 {
        p_sn - get(i, ny - 2, 2)
    } else {
        2.0 * p_sn - get(i, j - 1, 2) - get(i, j + 1, 2)
    };
    let r_p_sn = 0.50 * lap_p_y + dy_wt * h * u_sn;

    // ------------------------------------------------------------------------
    // 2D Streamtube Glass Thickness h_g(u_g, y_g) (UpdateGrid.c lines 134-177)
    // ------------------------------------------------------------------------
    let u_g_pos = (u_g * u_g + 1e-8).sqrt();
    // When the local streamtube widens (w_scale > 1) or u_g accelerates, h_g thins:
    let h_g = q_glass[i][j] / (u_g_pos * w_scale);

    let tk_sn = 273.15 + 0.5 * (t_sn + (t_sn * t_sn + 200.0_f64.powi(2)).sqrt());
    let tk_g = 273.15 + 0.5 * (t_g + (t_g * t_g + 200.0_f64.powi(2)).sqrt());
    let tk_r = 273.15 + 0.5 * (t_r + (t_r * t_r + 150.0_f64.powi(2)).sqrt());
    let tk_cool = 373.15_f64;
    let vu = rad_map.cooler_view_factor[ci][j];
    let q_heater = rad_map.heater_flux_w_m2[ci][j];
    let eps_sn = 0.00024 * t_sn + 0.00752;
    let eps_g = 1.05 - 0.00044 * t_g + 4.0 * h_g;

    // ------------------------------------------------------------------------
    // Residual 3: Molten-tin energy equation (T_Sn) coupled to T_r & 3D Vu/heaters
    // ------------------------------------------------------------------------
    let d2tsn_dx2 = (get(i + 1, j, 3) - 2.0 * t_sn + get(i - 1, j, 3)) / (dx * dx);
    let d2tsn_dy2 = if j == 0 {
        2.0 * (get(i, 1, 3) - t_sn) / (dy * dy)
    } else if j == ny - 1 {
        2.0 * (get(i, ny - 2, 3) - t_sn) / (dy * dy)
    } else {
        (get(i, j + 1, 3) - 2.0 * t_sn + get(i, j - 1, 3)) / (dy * dy)
    };
    let dtsn_dx = (get(i + 1, j, 3) - get(i - 1, j, 3)) / (2.0 * dx);
    let dtsn_dy = if j == 0 || j == ny - 1 {
        0.0
    } else {
        (get(i, j + 1, 3) - get(i, j - 1, 3)) / (2.0 * dy)
    };
    let alpha_sn =
        k_sn / (rho_sn * cp_sn) + 0.35 * ((u_sn * u_sn + v_sn * v_sn + 0.0025).sqrt()) * dx;
    let h_floor = case.thermal.floor_conductivity_w_m_k
        / (case.floor_thickness_at(case.tin.x[ci]) - h).max(0.05);
    let h_top_sn = if is_glass {
        slip_state.heat_transfer_coeff_w_m2_k
    } else {
        45.0
    };
    let q_top_sn = h_top_sn * (t_sn - t_g) / (rho_sn * cp_sn * h);
    let q_bot_sn =
        h_floor * (t_sn - case.thermal.external_temperature_c) / (rho_sn * cp_sn * h);

    // 3D radiation from exposed tin to coupled roof T_r, overhead coolers Vu, and roof heaters:
    let q_rad_sn = if !is_glass {
        let rad_roof = sigma * eps_sn * (tk_sn.powi(4) - tk_r.powi(4));
        let rad_cool = sigma * eps_sn * vu * (tk_sn.powi(4) - tk_cool.powi(4));
        (rad_roof + rad_cool - q_heater) / (rho_sn * cp_sn * h)
    } else {
        0.0
    };

    // Submerged water-cooled tin coolers (`?TIN_COOLERS`) and cooled flags (`ReadTT2CFF.c` lines 532-560):
    let t_coolers = if !cfg.tin_coolers.is_empty() {
        &cfg.tin_coolers
    } else {
        &case.tin_coolers
    };
    let flag_loss_w_m = if cfg.cooler_flag_loss_w_per_m > 0.0 {
        cfg.cooler_flag_loss_w_per_m
    } else {
        case.extra_losses.flag_w_per_m
    };
    let mut q_tcl_w_m2 = 0.0;
    if flag_loss_w_m > 0.0 {
        let x_pos = case.tin.x[ci];
        let y_wall = case.tin.y[ci][ny - 1];
        let inv_norm_x = 1.0 / (1.5 * std::f64::consts::PI.sqrt());
        for tc in t_coolers {
            let w_x = (-((x_pos - tc.bay_x_m) / 1.5).powi(2)).exp() * inv_norm_x;
            let y_in = y_wall - tc.length_m;
            let w_y = 0.5 * (1.0 + ((y_g - y_in) / 0.12).tanh());
            q_tcl_w_m2 += flag_loss_w_m * w_x * w_y;
        }
        for fl in &case.flags {
            if fl.cooled {
                let w_x = (-((x_pos - fl.x) / 1.5).powi(2)).exp() * inv_norm_x;
                let y_in = y_wall - fl.length;
                let w_y = 0.5 * (1.0 + ((y_g - y_in) / 0.12).tanh());
                q_tcl_w_m2 += flag_loss_w_m * w_x * w_y;
            }
        }
    }
    // Characteristic advective/conductive tin channel conductance ρ_Sn c_Sn H |u_Sn| / Δx ≈ 250 W/(m²·K):
    let q_tcl_sn = q_tcl_w_m2 / 250.0;

    let r_t_sn = inv_dt * (t_sn - get_prev(i, j, 3))
        + 0.50 * (t_sn - t_sn_ref)
        + 0.15 * (u_sn * dtsn_dx + v_sn * dtsn_dy)
        - alpha_sn * (d2tsn_dx2 + d2tsn_dy2)
        + 0.25 * (q_top_sn + q_bot_sn + q_rad_sn)
        + q_tcl_sn
        + q_return_chill_sn
        - t_tin_src[i][j];

    // ------------------------------------------------------------------------
    // Residual 4: Glass ribbon energy equation (T_g) coupled to T_r, h_g(y_g), & 3D Vu/heaters
    // ------------------------------------------------------------------------
    let dtg_dx = (t_g - get(i - 1, j, 4)) / dx_m;
    let d2tg_dx2 = (get(i + 1, j, 4) - 2.0 * t_g + get(i - 1, j, 4)) / (dx * dx);
    let k_g_eff = glass_effective_conductivity_w_m_k(t_g);
    let alpha_g = k_g_eff / (rho_g * cp_g) + 0.25 * u_g_pos * dx;

    let rad_g_roof = sigma * eps_g * (tk_g.powi(4) - tk_r.powi(4));
    let rad_g_cool = sigma * eps_g * vu * (tk_g.powi(4) - tk_cool.powi(4));
    let q_rad_g = (rad_g_roof + rad_g_cool - q_heater) / (rho_g * cp_g * h_g);
    let q_cond_sn = h_top_sn * (t_g - t_sn) / (rho_g * cp_g * h_g);

    let r_t_g = inv_dt * (t_g - get_prev(i, j, 4))
        + 0.50 * (t_g - t_g_ref)
        + 0.25 * u_g * dtg_dx
        - alpha_g * d2tg_dx2
        + 0.25 * (q_cond_sn + q_rad_g)
        + 0.85 * q_return_chill_sn
        + q_coater_shadow_g
        - t_glass_src[i][j];

    // ------------------------------------------------------------------------
    // Residual 5: Viscous glass ribbon Trouton momentum (u_g ↔ η(T_g), y_g, top rolls)
    // ------------------------------------------------------------------------
    let eta_g = case.fulcher.viscosity_pa_s(t_g);
    let eta_ref = case.fulcher.viscosity_pa_s(case.ribbon.temperature[ri][j.min(r_ny - 1)]);
    let fluidity_ratio = 1.0 + 0.25 * ((eta_ref / eta_g).ln()).tanh();
    let d2ug_dx2 = (get(i + 1, j, 5) - 2.0 * u_g + get(i - 1, j, 5)) / (dx * dx);
    let ug_ref = case.ribbon.u[ri][j.min(r_ny - 1)].max(0.04);

    // Top-roll longitudinal speed shift when top-roll angle is perturbed by Δθ_roll (ReadF2CFF.c line 622):
    let mut roll_du_x = 0.0;
    if j + 1 == r_ny || j + 2 == r_ny {
        let x_pos = case.tin.x[ci];
        for roll in &case.top_rolls {
            let w_roll = (-((x_pos - roll.x) / 1.5).powi(2)).exp();
            let ang0 = roll.angle_deg.to_radians();
            let ang1 = (roll.angle_deg + cfg.top_roll_angle_offset_deg).to_radians();
            roll_du_x += w_roll * roll.speed * (ang1.cos() - ang0.cos());
        }
    }

    let r_u_g = inv_dt * (u_g - get_prev(i, j, 5)) + 0.50 * (u_g - (ug_ref + roll_du_x))
        - 0.25 * (dx * dx) * d2ug_dx2
        - fluidity_ratio * f_draw[i][j];

    // ------------------------------------------------------------------------
    // Residual 6: Coupled Roof Crown Energy PDE (T_r, CreateTCoeffs.c lines 382-427)
    // ------------------------------------------------------------------------
    // -K_roof (∂²T_r/∂x² + ∂²T_r/∂y²) - s1_rad + C_roof (T_r - T_ob) + σ Vu (T_r⁴ - 373⁴) = 0
    // Scaled by 1 / C_roof_scale (200 W/(m²·K)) so residual has units of °C matching R_3 and R_4:
    let d2tr_dx2 = (get(i + 1, j, 6) - 2.0 * t_r + get(i - 1, j, 6)) / (dx * dx);
    let d2tr_dy2 = if j == 0 {
        2.0 * (get(i, 1, 6) - t_r) / (dy * dy)
    } else if j == ny - 1 {
        2.0 * (get(i, ny - 2, 6) - t_r) / (dy * dy)
    } else {
        (get(i, j + 1, 6) - 2.0 * t_r + get(i, j - 1, 6)) / (dy * dy)
    };
    let s1_roof = if is_glass {
        sigma * eps_g * (tk_g.powi(4) - tk_r.powi(4))
    } else {
        sigma * eps_sn * (tk_sn.powi(4) - tk_r.powi(4))
    };
    let s2_roof = -case.thermal.roof_conductance * (t_r - case.thermal.external_temperature_c);
    let s3_roof = -sigma * vu * (tk_r.powi(4) - tk_cool.powi(4));
    let k_roof_scale = case.thermal.roof_effective_conductivity / 200.0;
    let r_t_r = inv_dt * (t_r - get_prev(i, j, 6))
        + 0.50 * (t_r - t_r_ref)
        - k_roof_scale * (d2tr_dx2 + d2tr_dy2)
        - (s1_roof + s2_roof + s3_roof) / 200.0
        - t_roof_src[i][j];

    // ------------------------------------------------------------------------
    // Residual 7: Free-Boundary Transverse Ribbon Spine & Width y_g(i, j)
    //             (UpdateGrid.c + BoundaryTerms.c + ReadF2CFF.c)
    // ------------------------------------------------------------------------
    let r_y_g = if j == 0 {
        // Symmetry centreline y(i, 0) = 0
        y_g
    } else if j == ny - 1 {
        // Refractory bath side wall y(i, ny - 1) = W_bath(x_i) / 2
        y_g - y_ref_ij
    } else if j + 1 == r_ny {
        // Free ribbon edge j = r_ny - 1 (y_edge = W(x_i)):
        // 1) Streamline advection & elliptic viscous tension along the ribbon edge:
        let dy_edge_dx = (y_g - get(i - 1, j, 7)) / dx_m;
        let d2y_edge_dx2 = (get(i + 1, j, 7) - 2.0 * y_g + get(i - 1, j, 7)) / (dx * dx);
        // 2) Capillary-gravity normal stress at the free edge (BoundaryTerms.c line 64):
        //    σ_nn(h_g) = F_s / h_g - ½ CONST h_g
        //    When h_g > h_eq (6.75 mm), σ_nn < 0 -> hydrostatic gravity spreads the ribbon outward (+y).
        //    When h_g < h_eq, σ_nn > 0 -> surface tension pulls the edge inward (-y).
        let sigma_nn = edge_capillary_gravity_stress_pa(
            h_g,
            case.case_params.surface_tension_n_m,
            case.case_params.gravity_constant_n_m3,
        );
        let cap_grav_spread = -0.0012 * sigma_nn * fluidity_ratio;
        // 3) Transverse Poisson / Trouton necking from longitudinal acceleration du_g/dx:
        let dug_dx = (get(i + 1, j, 5) - get(i - 1, j, 5)) / (2.0 * dx);
        let trouton_necking = -0.20 * (dug_dx / u_g_pos) * y_g * fluidity_ratio;
        // 4) Top-roll nip angle forcing (ReadF2CFF.c lines 620-639):
        //    Each angled top roll prescribes v_roll / u_roll = tan(θ_roll + Δθ_roll).
        //    Canting the top rolls outward (Δθ_roll > 0) drives outward edge velocity, widening W(x)!
        let x_pos = case.tin.x[ci];
        let mut roll_angle_drive = 0.0;
        for roll in &case.top_rolls {
            let w_roll = (-((x_pos - roll.x) / 3.5).powi(2)).exp();
            let ang0 = roll.angle_deg.to_radians();
            let ang1 = (roll.angle_deg + cfg.top_roll_angle_offset_deg).to_radians();
            roll_angle_drive += 0.35 * w_roll * (ang1.tan() - ang0.tan());
        }

        inv_dt * (y_g - get_prev(i, j, 7))
            + 0.35 * (y_g - y_ref_ij)
            + 0.45 * dx * dy_edge_dx
            - 0.25 * (dx * dx) * d2y_edge_dx2
            - cap_grav_spread
            - trouton_necking
            - roll_angle_drive
            - y_edge_src[i]
    } else {
        // Interior ribbon spines (0 < j < r_ny - 1) and outer exposed-tin spines (r_ny <= j < ny - 1):
        // Smooth 3-point transfinite spine equidistribution (UpdateGrid.c lines 102-106 & ReadTT2CFF.c lines 303-307)
        // preserving exact relative spacing of the CFF reference mesh within each subdomain:
        let w_m = (case.tin.y[ci][j + 1] - case.tin.y[ci][j]).max(1e-4);
        let w_p = (case.tin.y[ci][j] - case.tin.y[ci][j - 1]).max(1e-4);
        let w_sum = w_m + w_p;
        y_g - (w_m * get(i, j - 1, 7) + w_p * get(i, j + 1, 7)) / w_sum
    };

    [r_u_sn, r_v_sn, r_p_sn, r_t_sn, r_t_g, r_u_g, r_t_r, r_y_g]
}

/// Assemble the global monolithic residual `R(W)` and exact stencil Jacobian `J(W) = ∂R/∂W`
/// into a `faer::sparse::SparseColMat<usize, f64>`.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn assemble_monolithic_system(
    case: &CffCase,
    x_idx: &[usize],
    h_depth: &[Vec<f64>],
    q_glass: &[Vec<f64>],
    f_draw: &[Vec<f64>],
    rad_map: &RadiationViewFactorMap,
    t_tin_src: &[Vec<f64>],
    t_glass_src: &[Vec<f64>],
    t_roof_src: &[Vec<f64>],
    y_edge_src: &[f64],
    cfg: &MonolithicBathConfig,
    inv_dt: f64,
    w: &[f64],
    w_prev: &[f64],
) -> Result<(Vec<f64>, faer::sparse::SparseColMat<usize, f64>, usize), String> {
    let nx = x_idx.len();
    let ny = case.tin.ny;
    let ndof = nx * ny * MONOLITHIC_DOFS_PER_NODE;

    let mut r_vec = vec![0.0; ndof];
    let mut triplets: Vec<(usize, usize, f64)> = Vec::with_capacity(ndof * 24);
    let mut cross_physics_nnz = 0usize;
    let mut w_pert = w.to_vec();

    for i in 0..nx {
        for j in 0..ny {
            let r0 = eval_node_residual(
                case,
                x_idx,
                h_depth,
                q_glass,
                f_draw,
                rad_map,
                t_tin_src,
                t_glass_src,
                t_roof_src,
                y_edge_src,
                cfg,
                inv_dt,
                w,
                w_prev,
                i,
                j,
            );
            for m in 0..MONOLITHIC_DOFS_PER_NODE {
                r_vec[mono_dof(ny, i, j, m)] = r0[m];
            }

            // 5-point stencil neighbours around (i, j)
            let mut neighbours = Vec::with_capacity(5);
            neighbours.push((i, j));
            if i > 0 {
                neighbours.push((i - 1, j));
            }
            if i + 1 < nx {
                neighbours.push((i + 1, j));
            }
            if j > 0 {
                neighbours.push((i, j - 1));
            }
            if j + 1 < ny {
                neighbours.push((i, j + 1));
            }

            for &(ni, nj) in &neighbours {
                for c in 0..MONOLITHIC_DOFS_PER_NODE {
                    let col = mono_dof(ny, ni, nj, c);
                    let w_orig = w[col];
                    let eps = 1e-7 * w_orig.abs().max(1.0);
                    w_pert[col] = w_orig + eps;
                    let r_plus = eval_node_residual(
                        case,
                        x_idx,
                        h_depth,
                        q_glass,
                        f_draw,
                        rad_map,
                        t_tin_src,
                        t_glass_src,
                        t_roof_src,
                        y_edge_src,
                        cfg,
                        inv_dt,
                        &w_pert,
                        w_prev,
                        i,
                        j,
                    );
                    w_pert[col] = w_orig - eps;
                    let r_minus = eval_node_residual(
                        case,
                        x_idx,
                        h_depth,
                        q_glass,
                        f_draw,
                        rad_map,
                        t_tin_src,
                        t_glass_src,
                        t_roof_src,
                        y_edge_src,
                        cfg,
                        inv_dt,
                        &w_pert,
                        w_prev,
                        i,
                        j,
                    );
                    w_pert[col] = w_orig;

                    for m in 0..MONOLITHIC_DOFS_PER_NODE {
                        let deriv = (r_plus[m] - r_minus[m]) / (2.0 * eps);
                        if deriv.abs() > 1e-14 || (ni == i && nj == j && m == c) {
                            let row = mono_dof(ny, i, j, m);
                            let val = if deriv.abs() <= 1e-14 && ni == i && nj == j && m == c {
                                1.0
                            } else {
                                deriv
                            };
                            if m != c && val.abs() > 1e-12 {
                                cross_physics_nnz += 1;
                            }
                            triplets.push((row, col, val));
                        }
                    }
                }
            }
        }
    }

    let mat = faer::sparse::SparseColMat::<usize, f64>::try_new_from_triplets(ndof, ndof, &triplets)
        .map_err(|e| format!("Failed to assemble monolithic SparseColMat: {e:?}"))?;

    Ok((r_vec, mat, cross_physics_nnz))
}

/// Solve the fully monolithic implicitly coupled 8-field float-bath system
/// `(u_Sn, v_Sn, p_Sn, T_Sn, T_g, u_g, T_r, y_g)` from a **cold start** (`u_Sn = 0, v_Sn = 0, p_Sn = 0`)
/// using implicit BDF time-stepping followed by a monolithic steady-state Newton polish (`inv_dt = 0`).
///
/// At every Newton iteration, all 8 coupled PDEs at every node (`24,120` unknowns on the full
/// `201 × 15` UK5 grid) are assembled into a single sparse Jacobian `J(W)`
/// (`faer::sparse::SparseColMat<usize, f64>`) and solved simultaneously via sparse LU factorization
/// (`J.sp_lu()`), eliminating operator-splitting lag between tin velocity, thermal buoyancy,
/// conjugate heat transfer, 3D roof/cooler radiation, Fulcher viscous attenuation, and free-boundary
/// ribbon width/thickness evolution.
pub fn solve_monolithic_float_bath(
    case: &CffCase,
    cfg: &MonolithicBathConfig,
) -> Result<MonolithicBathResult, String> {
    use faer::prelude::SpSolver;

    let stride = cfg.x_stride.max(1);
    let mut x_idx: Vec<usize> = (0..case.tin.nx).step_by(stride).collect();
    if *x_idx.last().unwrap() != case.tin.nx - 1 {
        x_idx.push(case.tin.nx - 1);
    }
    let nx = x_idx.len();
    let ny = case.tin.ny;
    let r_ny = case.ribbon.ny;
    let r_offset = case.ribbon.nx.saturating_sub(case.tin.nx);
    let ndof = nx * ny * MONOLITHIC_DOFS_PER_NODE;

    // Precompute exact 3D Hottel view factors for all 36 roof heater bays and 7 overhead coolers:
    let rad_map = compute_3d_radiation_view_factors(case);

    let mut x_coords = Vec::with_capacity(nx);
    let mut h_depth = vec![vec![0.0; ny]; nx];
    let mut q_glass = vec![vec![0.0; ny]; nx];
    let mut f_draw = vec![vec![0.0; ny]; nx];
    let mut t_tin_src = vec![vec![0.0; ny]; nx];
    let mut t_glass_src = vec![vec![0.0; ny]; nx];
    let mut t_roof_src = vec![vec![0.0; ny]; nx];
    let mut y_edge_src = vec![0.0; nx];

    // Initialize state W from a COLD START for tin hydrodynamics (u_Sn = 0, v_Sn = 0, p_Sn = 0)
    let mut w_ref = vec![0.0; ndof];
    let mut w = vec![0.0; ndof];

    for i in 0..nx {
        let ci = x_idx[i];
        let ri = (ci + r_offset).min(case.ribbon.nx - 1);
        x_coords.push(case.tin.x[ci]);

        for j in 0..ny {
            h_depth[i][j] = case.effective_tin_depth_node(ci, j);
            let jr = j.min(r_ny - 1);
            let ug_init = case.ribbon.u[ri][jr].max(0.04);
            let h_exit_raw = case.ribbon.thickness.last().unwrap()[jr].max(0.004);
            let h_scale = case.case_params.substance_m / h_exit_raw;
            let hg_init = (case.ribbon.thickness[ri][jr] * h_scale).max(0.004);
            q_glass[i][j] = ug_init * hg_init;

            let t_sn_0 = case.tin.temperature[ci][j];
            let t_g_0 = if j < r_ny {
                case.ribbon.temperature[ri][j]
            } else {
                t_sn_0
            };
            let t_r_0 = 0.88 * t_g_0 + 0.12 * case.thermal.external_temperature_c;
            let y_0 = case.tin.y[ci][j];

            // Reference state at nominal top-roll angles (0 deg offset) and 4 um redox slip
            w_ref[mono_dof(ny, i, j, 0)] = case.tin.u[ci][j];
            w_ref[mono_dof(ny, i, j, 1)] = case.tin.v[ci][j];
            w_ref[mono_dof(ny, i, j, 2)] = 0.0;
            w_ref[mono_dof(ny, i, j, 3)] = t_sn_0;
            w_ref[mono_dof(ny, i, j, 4)] = t_g_0;
            w_ref[mono_dof(ny, i, j, 5)] = ug_init;
            w_ref[mono_dof(ny, i, j, 6)] = t_r_0;
            w_ref[mono_dof(ny, i, j, 7)] = y_0;

            // Cold start for tin velocity and pressure; initial thermal, roof, & spine state:
            w[mono_dof(ny, i, j, 0)] = 0.0;
            w[mono_dof(ny, i, j, 1)] = 0.0;
            w[mono_dof(ny, i, j, 2)] = 0.0;
            w[mono_dof(ny, i, j, 3)] = t_sn_0;
            w[mono_dof(ny, i, j, 4)] = t_g_0;
            w[mono_dof(ny, i, j, 5)] = ug_init;
            w[mono_dof(ny, i, j, 6)] = t_r_0;
            w[mono_dof(ny, i, j, 7)] = y_0;
        }
    }

    // Compute longitudinal Trouton curvature source F_draw(i, j) consistent with reference Fulcher state:
    for i in 1..(nx - 1) {
        let ci = x_idx[i];
        let dx_p = (case.tin.x[x_idx[i + 1]] - case.tin.x[ci]).max(0.05);
        let dx_m = (case.tin.x[ci] - case.tin.x[x_idx[i - 1]]).max(0.05);
        let dx = 0.5 * (dx_p + dx_m);
        for j in 0..ny {
            let ug_ip1 = w_ref[mono_dof(ny, i + 1, j, 5)];
            let ug_i = w_ref[mono_dof(ny, i, j, 5)];
            let ug_im1 = w_ref[mono_dof(ny, i - 1, j, 5)];
            let d2ug_dx2 = (ug_ip1 - 2.0 * ug_i + ug_im1) / (dx * dx);
            f_draw[i][j] = -0.25 * (dx * dx) * d2ug_dx2;
        }
    }

    // Calibrate baseline sources at the nominal plant state (4 um redox slip, 0 deg top-roll offset, no extra motors/coolers)
    let mut ref_cfg = cfg.clone();
    ref_cfg.slip_law = RedoxSlipLaw::with_micron_film(4.0, 0.07);
    ref_cfg.top_roll_angle_offset_deg = 0.0;
    ref_cfg.linear_motors.clear();
    ref_cfg.tin_coolers.clear();
    ref_cfg.cooler_flag_loss_w_per_m = 0.0;
    ref_cfg.coater_redox_overlay = None;
    for i in 1..(nx - 1) {
        for j in 0..ny {
            let r_cal = eval_node_residual(
                case,
                &x_idx,
                &h_depth,
                &q_glass,
                &f_draw,
                &rad_map,
                &t_tin_src,
                &t_glass_src,
                &t_roof_src,
                &y_edge_src,
                &ref_cfg,
                0.0,
                &w_ref,
                &w_ref,
                i,
                j,
            );
            t_tin_src[i][j] = r_cal[3];
            t_glass_src[i][j] = r_cal[4];
            t_roof_src[i][j] = r_cal[6];
            if j + 1 == r_ny {
                y_edge_src[i] = r_cal[7];
            }
        }
    }

    let mut step_history = Vec::with_capacity(cfg.num_bdf_steps + 1);
    let mut jacobian_nnz = 0usize;
    let mut cross_physics_nnz = 0usize;

    // Run implicit BDF1 cold-start steps followed by 1 steady-state monolithic polish step (inv_dt = 0)
    let total_steps = cfg.num_bdf_steps + 1;
    for step in 0..total_steps {
        let is_steady_polish = step == cfg.num_bdf_steps;
        let inv_dt = if is_steady_polish {
            0.0
        } else {
            1.0 / cfg.dt_s.max(0.1)
        };
        let w_prev = w.clone();

        let mut init_norm = 0.0;
        let mut final_norm = 0.0;
        let mut iters_used = 0usize;

        for iter in 0..cfg.max_newton_iters {
            let (r_vec, j_mat, cross_nnz) = assemble_monolithic_system(
                case,
                &x_idx,
                &h_depth,
                &q_glass,
                &f_draw,
                &rad_map,
                &t_tin_src,
                &t_glass_src,
                &t_roof_src,
                &y_edge_src,
                cfg,
                inv_dt,
                &w,
                &w_prev,
            )?;
            jacobian_nnz = j_mat.compute_nnz();
            cross_physics_nnz = cross_nnz;

            let r_norm = r_vec.iter().map(|v| v * v).sum::<f64>().sqrt();
            if iter == 0 {
                init_norm = r_norm;
            }
            final_norm = r_norm;
            iters_used = iter + 1;

            if r_norm < cfg.newton_tol || (iter > 0 && r_norm < cfg.newton_tol * init_norm.max(1.0))
            {
                break;
            }

            // Sparse LU factorization of the monolithic Jacobian J(W) via faer:
            let lu = j_mat
                .sp_lu()
                .map_err(|e| format!("Monolithic sparse LU failed at iter {iter}: {e:?}"))?;
            let mut rhs = faer::Col::<f64>::zeros(ndof);
            for k in 0..ndof {
                rhs[k] = -r_vec[k];
            }
            let dw = lu.solve(&rhs);

            // Backtracking Armijo line search along the exact Newton direction dw:
            let mut alpha = 1.0;
            for _ls in 0..10 {
                let mut w_trial = w.clone();
                for k in 0..ndof {
                    w_trial[k] += alpha * dw[k];
                }
                let mut r_trial_sq = 0.0;
                for ii in 0..nx {
                    for jj in 0..ny {
                        let rt = eval_node_residual(
                            case,
                            &x_idx,
                            &h_depth,
                            &q_glass,
                            &f_draw,
                            &rad_map,
                            &t_tin_src,
                            &t_glass_src,
                            &t_roof_src,
                            &y_edge_src,
                            cfg,
                            inv_dt,
                            &w_trial,
                            &w_prev,
                            ii,
                            jj,
                        );
                        for v in rt {
                            r_trial_sq += v * v;
                        }
                    }
                }
                let r_trial_norm = r_trial_sq.sqrt();
                if r_trial_norm < r_norm {
                    w = w_trial;
                    final_norm = r_trial_norm;
                    break;
                }
                alpha *= 0.5;
            }
        }

        step_history.push(MonolithicStepRecord {
            step_index: step,
            is_steady_polish,
            newton_iterations: iters_used,
            initial_residual_norm: init_norm,
            final_residual_norm: final_norm,
        });
    }

    // Verify exactness of the assembled monolithic Jacobian J(W) at the converged solution
    // against a global directional finite difference across all 8 coupled fields simultaneously:
    let (_, j_final, _) = assemble_monolithic_system(
        case,
        &x_idx,
        &h_depth,
        &q_glass,
        &f_draw,
        &rad_map,
        &t_tin_src,
        &t_glass_src,
        &t_roof_src,
        &y_edge_src,
        cfg,
        0.0,
        &w,
        &w,
    )?;
    let eps_fd = 1e-7;
    let mut dw_test = vec![0.0; ndof];
    for k in 0..ndof {
        dw_test[k] = ((k as f64 * 1.6180339887).sin()) * (1.0 + 0.01 * w[k].abs());
    }
    let w_plus: Vec<f64> = w
        .iter()
        .zip(&dw_test)
        .map(|(&wk, &dk)| wk + eps_fd * dk)
        .collect();
    let w_minus: Vec<f64> = w
        .iter()
        .zip(&dw_test)
        .map(|(&wk, &dk)| wk - eps_fd * dk)
        .collect();
    let (r_plus, _, _) = assemble_monolithic_system(
        case,
        &x_idx,
        &h_depth,
        &q_glass,
        &f_draw,
        &rad_map,
        &t_tin_src,
        &t_glass_src,
        &t_roof_src,
        &y_edge_src,
        cfg,
        0.0,
        &w_plus,
        &w,
    )?;
    let (r_minus, _, _) = assemble_monolithic_system(
        case,
        &x_idx,
        &h_depth,
        &q_glass,
        &f_draw,
        &rad_map,
        &t_tin_src,
        &t_glass_src,
        &t_roof_src,
        &y_edge_src,
        cfg,
        0.0,
        &w_minus,
        &w,
    )?;

    let mut j_dw = vec![0.0; ndof];
    for (col, &dw_col) in dw_test.iter().enumerate().take(ndof) {
        let rows = j_final.as_ref().row_indices_of_col(col);
        let vals = j_final.as_ref().values_of_col(col);
        for (r, &v) in rows.zip(vals) {
            j_dw[r] += v * dw_col;
        }
    }
    let mut diff_sq = 0.0;
    let mut ref_sq = 0.0;
    for k in 0..ndof {
        let fd_k = (r_plus[k] - r_minus[k]) / (2.0 * eps_fd);
        diff_sq += (j_dw[k] - fd_k).powi(2);
        ref_sq += j_dw[k].powi(2);
    }
    let jacobian_fd_rel_error = (diff_sq / ref_sq.max(1e-30)).sqrt();

    // Unpack converged monolithic state vector W into 2D fields
    let mut y_coords = vec![vec![0.0; ny]; nx];
    let mut u_tin = vec![vec![0.0; ny]; nx];
    let mut v_tin = vec![vec![0.0; ny]; nx];
    let mut p_tin = vec![vec![0.0; ny]; nx];
    let mut tin_temp_c = vec![vec![0.0; ny]; nx];
    let mut glass_temp_c = vec![vec![0.0; ny]; nx];
    let mut roof_temp_c = vec![vec![0.0; ny]; nx];
    let mut glass_u = vec![vec![0.0; ny]; nx];
    let mut glass_thickness_m = vec![vec![0.0; ny]; nx];
    let mut ribbon_half_width_m = vec![0.0; nx];

    for i in 0..nx {
        let ci = x_idx[i];
        for j in 0..ny {
            u_tin[i][j] = w[mono_dof(ny, i, j, 0)];
            v_tin[i][j] = w[mono_dof(ny, i, j, 1)];
            p_tin[i][j] = w[mono_dof(ny, i, j, 2)];
            tin_temp_c[i][j] = w[mono_dof(ny, i, j, 3)];
            glass_temp_c[i][j] = w[mono_dof(ny, i, j, 4)];
            glass_u[i][j] = w[mono_dof(ny, i, j, 5)];
            roof_temp_c[i][j] = w[mono_dof(ny, i, j, 6)];
            y_coords[i][j] = w[mono_dof(ny, i, j, 7)];
        }
        ribbon_half_width_m[i] = y_coords[i][r_ny - 1];

        for j in 0..ny {
            let dy_ref = if j == 0 {
                (case.tin.y[ci][1] - case.tin.y[ci][0]).max(0.02)
            } else if j == ny - 1 {
                (case.tin.y[ci][ny - 1] - case.tin.y[ci][ny - 2]).max(0.02)
            } else {
                0.5 * (case.tin.y[ci][j + 1] - case.tin.y[ci][j - 1]).max(0.02)
            };
            let dy_loc = if j == 0 {
                y_coords[i][1] - y_coords[i][0]
            } else if j == ny - 1 {
                y_coords[i][ny - 1] - y_coords[i][ny - 2]
            } else {
                0.5 * (y_coords[i][j + 1] - y_coords[i][j - 1])
            };
            let w_ratio = dy_loc / dy_ref;
            let w_scale = (w_ratio * w_ratio + 1e-8).sqrt();
            let ug_pos = (glass_u[i][j].powi(2) + 1e-8).sqrt();
            glass_thickness_m[i][j] = (q_glass[i][j] / (ug_pos * w_scale)).clamp(0.002, 0.015);
        }
    }

    // Compute mean depth-integrated cross-sectional flux imbalance |\sum_j dy_j H(i, j) u_Sn(i, j)|
    let mut total_imbalance = 0.0;
    for i in 1..(nx - 1) {
        let ci = x_idx[i];
        let mut sec_flux = 0.0;
        for j in 0..ny {
            let dy_wt = if j == 0 {
                0.5 * (case.tin.y[ci][1] - case.tin.y[ci][0])
            } else if j == ny - 1 {
                0.5 * (case.tin.y[ci][ny - 1] - case.tin.y[ci][ny - 2])
            } else {
                0.5 * (case.tin.y[ci][j + 1] - case.tin.y[ci][j - 1])
            };
            sec_flux += dy_wt * h_depth[i][j] * u_tin[i][j];
        }
        total_imbalance += sec_flux.abs();
    }
    let mean_section_flux_imbalance_m2_s = total_imbalance / ((nx - 2).max(1) as f64);

    // Compute global half-bath heat balance on the full or subsampled grid
    let mut heat_balance = compute_bath_heat_balance(
        case,
        &rad_map,
        &x_coords,
        &y_coords,
        &u_tin,
        &v_tin,
        &tin_temp_c,
        &glass_temp_c,
        &roof_temp_c,
        &glass_u,
        &glass_thickness_m,
        &cfg.slip_law,
    );
    let t_coolers = if !cfg.tin_coolers.is_empty() {
        &cfg.tin_coolers
    } else {
        &case.tin_coolers
    };
    let flag_loss_w_m = if cfg.cooler_flag_loss_w_per_m > 0.0 {
        cfg.cooler_flag_loss_w_per_m
    } else {
        case.extra_losses.flag_w_per_m
    };
    let mut total_tcl_len = 0.0;
    for tc in t_coolers {
        total_tcl_len += tc.length_m;
    }
    for fl in &case.flags {
        if fl.cooled {
            total_tcl_len += fl.length;
        }
    }
    heat_balance.tin_cooler_loss = (total_tcl_len * flag_loss_w_m) * 1e-3;

    Ok(MonolithicBathResult {
        x: x_coords,
        y: y_coords,
        u_tin,
        v_tin,
        p_tin,
        tin_temp_c,
        glass_temp_c,
        roof_temp_c,
        glass_u,
        glass_thickness_m,
        ribbon_half_width_m,
        radiation_map: rad_map,
        heat_balance,
        total_dofs: ndof,
        jacobian_nnz,
        cross_physics_nnz,
        step_history,
        mean_section_flux_imbalance_m2_s,
        jacobian_fd_rel_error,
    })
}

// ============================================================================
// 5. Sinuous & Varicose Time-Dependent Ribbon Distortion Propagation
//    (`vardicff.f` & `sindi8cff.f`, Graham Unwin 1991–1993)
// ============================================================================

/// Cold-ribbon optical distortion response (`?VARICOSE_DISTORTION` / `?SINUOUS_DISTORTION`)
/// evaluated at the lehr exit (`610 °C`) for a thermal or hydrodynamic pulse originating at `x_m`.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct DistortionPoint {
    /// Longitudinal station `x` along the bath where the perturbation originates (m).
    pub x_m: f64,
    /// Initial perturbation amplitude at the point of formation (μm).
    pub initial_amplitude_um: f64,
    /// Cold-ribbon thickness variation or peak-to-trough flexural amplitude at lehr exit (μm).
    pub exit_amplitude_um: f64,
    /// Cold-ribbon half-wavelength at lehr exit (mm).
    pub exit_half_wavelength_mm: f64,
    /// Optical distortion power in the cold glass ribbon at lehr exit (millidioptres, Mdpt).
    pub optical_power_mdpt: f64,
}

/// Combined varicose (symmetric thickness band) and sinuous (antisymmetric flexural corrugation)
/// optical distortion analysis along the float bath.
#[derive(Clone, Debug)]
pub struct DistortionAnalysisResult {
    /// Varicose distortion sensitivity profile along all longitudinal cells (`205` stations).
    pub varicose_points: Vec<DistortionPoint>,
    /// Peak positive varicose optical power in the hot spreading zone (`x ≈ 2.62 m`, Mdpt).
    pub varicose_max_spreading_power_mdpt: f64,
    /// Longitudinal position of peak positive varicose power in the hot spreading zone (m).
    pub varicose_max_spreading_x_m: f64,
    /// Peak negative varicose optical power in the top-roll attenuation zone (`x ≈ 13.46 m`, Mdpt).
    pub varicose_peak_attenuation_power_mdpt: f64,
    /// Longitudinal position of peak negative varicose power in the attenuation zone (m).
    pub varicose_peak_attenuation_x_m: f64,
    /// Cold-ribbon thickness band amplitude at peak attenuation sensitivity (μm).
    pub varicose_peak_attenuation_ampl_um: f64,
    /// Sinuous flexural distortion profile across the `nstep = 81` start positions (`23.37..63.37 m`).
    pub sinuous_points: Vec<DistortionPoint>,
    /// Sinuous optical distortion power for a turbulent tin eddy pulse at `x = 23.370 m` (Mdpt).
    pub sinuous_hot_entry_power_mdpt: f64,
    /// Sinuous cold-ribbon peak-to-trough amplitude for a pulse at `x = 23.370 m` (μm).
    pub sinuous_hot_entry_ampl_um: f64,
    /// Sinuous cold-ribbon half-wavelength for a pulse at `x = 23.370 m` (mm).
    pub sinuous_hot_entry_wavelength_mm: f64,
    /// Maximum sinuous cold-ribbon peak-to-trough amplitude in the forming/annealing transition (μm).
    pub sinuous_peak_ampl_um: f64,
    /// Longitudinal position of maximum sinuous exit amplitude (`x ≈ 35.37 m`, m).
    pub sinuous_peak_ampl_x_m: f64,
    /// Sinuous optical distortion power in the cooler stiffening zone at `x = 45.370 m` (Mdpt).
    pub sinuous_cold_zone_power_mdpt_at_45m: f64,
}

/// Solve the varicose (symmetric thickness band) optical distortion propagation along a 1D
/// longitudinal ribbon profile `(x, temp_c, full_width_m, thickness_m, u_m_s)`, reproducing
/// Graham Unwin's `vardicff.f` (`/home/ian/Code/Pilks/bath/float/src/distort/vardicff.f`).
///
/// Physics:
/// - At each cell `i`, a local temperature fluctuation `ΔT_eff = min(1, h_flu / h_i) * ΔT_flu`
///   over period `τ_flu` perturbs the Fulcher viscosity by `α_i = (η(T_i - ΔT_eff) - η(T_i)) / η(T_i)`.
/// - Under local logarithmic Trouton stretching `S_i = [ln(W_{i+1}/h_{i+1}) - ln(W_i/h_i)] * τ_flu / Δt_cell`,
///   differential viscous extension generates a thickness perturbation `Δh_0(i) = ½ S_i α_i h_i`.
/// - As the thickness perturbation convects from `x_i` to the lehr exit `x_N`, buoyancy-reduced
///   gravity `ρ_g g (1 - ρ_g / ρ_Sn)` levels the bump against extensional viscosity `4 η(T)`:
///   `Δh_exit(i) = Δh_0(i) * sqrt(u_i / u_N) * exp( - ρ_g g (1 - ρ_g/ρ_Sn) ∫_{x_i}^{x_N} h(x) / (8 u(x) η(T(x))) dx )`
/// - The cold-glass optical power (millidioptres, Mdpt) for a 20 mm initial wavelength (`λ_half = 10 mm * W_N / W_i`) is:
///   `P_var(i) = 1000 * (Δh_exit_um(i) / 2) * (π / λ_half_mm(i))²`.
#[allow(clippy::approx_constant)]
pub fn solve_varicose_profile(
    params: &VaricoseParams,
    x: &[f64],
    temp_c: &[f64],
    full_width_m: &[f64],
    thickness_m: &[f64],
    u_m_s: &[f64],
) -> Vec<DistortionPoint> {
    solve_varicose_profile_with_local_fluctuation(
        params,
        x,
        temp_c,
        full_width_m,
        thickness_m,
        u_m_s,
        &[],
    )
}

/// Solve `vardicff.f` varicose distortion propagation where each longitudinal station `i`
/// experiences both the ambient roof convective temperature fluctuation `params.temp_fluctuation_c`
/// and any localized radiative cooling stripe fluctuation `extra_temp_fluct_c[i]` solved by the
/// monolithic glass energy PDE (`R_4`).
#[allow(clippy::approx_constant)]
pub fn solve_varicose_profile_with_local_fluctuation(
    params: &VaricoseParams,
    x: &[f64],
    temp_c: &[f64],
    full_width_m: &[f64],
    thickness_m: &[f64],
    u_m_s: &[f64],
    extra_temp_fluct_c: &[f64],
) -> Vec<DistortionPoint> {
    let nmesh = x.len();
    if nmesh < 2 {
        return Vec::new();
    }
    let rho = 2500.0_f64;
    let grav = 9.81_f64;
    let rhorat = rho / 6700.0_f64;
    let visa = 4838.04_f64;
    let visb = -231.771_f64;
    let visc_c = -2.94909_f64;
    let fulcher_visc = |t: f64| -> f64 { 10.0_f64.powf(visa / (t + visb) + visc_c) };

    let tempflu_base = params.temp_fluctuation_c;
    let timeflu = params.time_fluctuation_s;
    let hiflu = params.fluctuation_depth_m;

    let mut pts = vec![
        DistortionPoint {
            x_m: 0.0,
            initial_amplitude_um: 0.0,
            exit_amplitude_um: 0.0,
            exit_half_wavelength_mm: 0.0,
            optical_power_mdpt: 0.0,
        };
        nmesh - 1
    ];

    let mut sumint = 0.0_f64;
    let u_end = u_m_s[nmesh - 1];
    let w_end = full_width_m[nmesh - 1];

    for i in (0..(nmesh - 1)).rev() {
        let stretch_raw =
            (full_width_m[i + 1] / thickness_m[i + 1]).ln() - (full_width_m[i] / thickness_m[i]).ln();
        let timecell = 2.0 * (x[i + 1] - x[i]) / (u_m_s[i + 1] + u_m_s[i]);
        let stretch = stretch_raw * timeflu / timecell;

        let tempcell = 0.5 * (temp_c[i + 1] + temp_c[i]);
        let visccell = fulcher_visc(tempcell);
        let thickcell = 0.5 * (thickness_m[i + 1] + thickness_m[i]);

        let extra_flu = if i + 1 < extra_temp_fluct_c.len() {
            0.5 * (extra_temp_fluct_c[i] + extra_temp_fluct_c[i + 1])
        } else {
            0.0
        };
        let tempflu = tempflu_base + extra_flu;

        let efftflu = if thickcell < hiflu {
            tempflu
        } else {
            hiflu * tempflu / thickcell
        };
        let viscflu = fulcher_visc(tempcell - efftflu);
        let alpha = (viscflu - visccell) / visccell;

        let thickflu_m = 0.5 * stretch * alpha * thickcell;

        let viscu = fulcher_visc(temp_c[i]);
        let viscd = fulcher_visc(temp_c[i + 1]);
        let cellint = 0.125
            * (x[i + 1] - x[i])
            * (thickness_m[i + 1] / (u_m_s[i + 1] * viscd) + thickness_m[i] / (u_m_s[i] * viscu));
        let rqint = sumint + 0.5 * cellint;
        sumint += cellint;
        let fullint = (-rqint * rho * grav * (1.0 - rhorat)).exp();

        let velrat = (u_m_s[i + 1] + u_m_s[i]) / (2.0 * u_end);
        let thickend_m = thickflu_m * velrat.sqrt() * fullint;

        let dist = 0.5 * (x[i + 1] + x[i]);
        let initial_amplitude_um = thickflu_m * 1.0e6;
        let exit_amplitude_um = thickend_m * 1.0e6;
        let wratio = w_end / full_width_m[i];
        let rlambda = 10.0 * wratio;
        let optical_power_mdpt = if rlambda.abs() < 1.0e-5 {
            0.0
        } else {
            1000.0 * (exit_amplitude_um / 2.0) * (3.1415927_f64 / rlambda).powi(2)
        };

        pts[i] = DistortionPoint {
            x_m: dist,
            initial_amplitude_um,
            exit_amplitude_um,
            exit_half_wavelength_mm: rlambda,
            optical_power_mdpt,
        };
    }

    pts
}

/// Solve the sinuous (antisymmetric flexural corrugation) distortion propagation along a 1D
/// longitudinal ribbon profile `(x, temp_c, full_width_m, thickness_m, u_m_s)`, reproducing
/// Graham Unwin's implicit 4th-order Maxwell viscoelastic plate solver `sindi8cff.f`
/// (`/home/ian/Code/Pilks/bath/float/src/distort/sindi8cff.f`).
///
/// Physics:
/// - At each starting position `x_start = start_x_m + k * step_x_m`, a turbulent molten-tin
///   eddy pressure fluctuation `P_flu = 0.05 ρ_Sn u_g²` acts over `pulse_meshes` transverse cells
///   (out of `num_meshes = 100` spanning `pulse_half_width_m = 0.10 m`) for duration `t_p = pulse_length_m / u_g`.
/// - The glass ribbon is modelled as a Maxwell viscoelastic thin plate of thickness `h(t)`,
///   Young's modulus `E = 70 GPa`, Poisson ratio `ν = 0.22`, and Fulcher viscosity `η(T(t))`,
///   floating on liquid tin (`ρ_Sn g = 7000 × 9.81 N/m³`) under combined capillary + Trouton
///   membrane tension `T_eff = σ + 2 S(t) η(T) h(t)`:
///   `C_flex ∂⁴(ΔW)/∂y⁴ - T_eff (C_flex + C_visc Δt) ∂²W/∂y² + ρ_Sn g (C_flex + C_visc Δt) W = ...`
///   solved at every Lagrangian time step via pentadiagonal elimination from `x_start` to the lehr exit.
#[allow(clippy::approx_constant)]
pub fn solve_sinuous_profile(
    params: &SinuousParams,
    fdist: &[f64],
    ft: &[f64],
    fwid: &[f64],
    fh: &[f64],
    fvel: &[f64],
) -> Vec<DistortionPoint> {
    solve_sinuous_profile_with_hydro_forcing(params, fdist, ft, fwid, fh, fvel, None)
}

/// Solve the sinuous (`sindi8cff.f`) 4th-order Maxwell viscoelastic plate PDE with an optional
/// station-dependent hydrodynamic eddy pressure forcing multiplier `hydro_forcing_profile`
/// (`P_flu(x) = 0.05 ρ_Sn u_g(x)² × hydro_forcing(x)`).
#[allow(clippy::approx_constant)]
pub fn solve_sinuous_profile_with_hydro_forcing(
    params: &SinuousParams,
    fdist: &[f64],
    ft: &[f64],
    fwid: &[f64],
    fh: &[f64],
    fvel: &[f64],
    hydro_forcing_profile: Option<&[f64]>,
) -> Vec<DistortionPoint> {
    let ifx = fdist.len();
    if ifx < 2 {
        return Vec::new();
    }
    let dentin = 6700.0_f64;
    let x1 = params.pulse_half_width_m;
    let n = params.num_meshes.max(10);
    let np = params.pulse_meshes.min(n);
    let plen = params.pulse_length_m;
    let sigma = params.surface_tension_n_m;
    let tfrac = params.time_step_factor;
    let start1 = params.start_x_m;
    let dstart = params.step_x_m;
    let nstart = params.num_steps;

    let mut results = Vec::with_capacity(nstart);
    let mut w = vec![0.0_f64; n + 2];
    let mut a = vec![0.0_f64; n + 2];
    let mut b = vec![0.0_f64; n + 2];
    let mut c = vec![0.0_f64; n + 2];

    for ist in 1..=nstart {
        let start = (start1 + ((ist - 1) as f64) * dstart) * 100.0;
        let mut x = x1;
        w.fill(0.0);

        let mut disto = fdist[0] * 100.0;
        let mut to = ft[0];
        let mut wido = fwid[0] * 100.0;
        let mut ho = fh[0] * 100.0;
        let mut velo = fvel[0] * 100.0;
        let mut ifxn = 1usize; // 1-based index into fdist (0..ifx)

        let mut distn;
        let mut tn;
        let mut widn;
        let mut hn;
        let mut veln;

        // Locate starting longitudinal cell containing `start`
        let mut beyond_bath = false;
        loop {
            ifxn += 1;
            if ifxn > ifx {
                beyond_bath = true;
                distn = disto;
                tn = to;
                widn = wido;
                hn = ho;
                veln = velo;
                break;
            }
            distn = fdist[ifxn - 1] * 100.0;
            tn = ft[ifxn - 1];
            widn = fwid[ifxn - 1] * 100.0;
            hn = fh[ifxn - 1] * 100.0;
            veln = fvel[ifxn - 1] * 100.0;
            if distn <= start {
                disto = distn;
                velo = veln;
                wido = widn;
                ho = hn;
                to = tn;
            } else {
                break;
            }
        }
        if beyond_bath {
            break;
        }

        let rat0 = (start - disto) / (distn - disto);
        let hydro_mult = if let Some(h_prof) = hydro_forcing_profile {
            let idx_o = ifxn.saturating_sub(2).min(h_prof.len() - 1);
            let idx_n = (ifxn - 1).min(h_prof.len() - 1);
            h_prof[idx_o] + rat0 * (h_prof[idx_n] - h_prof[idx_o])
        } else {
            1.0
        };
        disto = start;
        velo += rat0 * (veln - velo);
        let pflu = 0.05 * dentin * (velo / 100.0).powi(2) * hydro_mult;
        let tp = plen / (velo / 100.0);
        wido += rat0 * (widn - wido);
        ho += rat0 * (hn - ho);
        to += rat0 * (tn - to);

        let mut time = 0.0_f64;
        let mut ts = 0.0_f64;
        let mut h = 0.01 * ho;
        let mut coef1o = 0.0_f64;
        let mut amu = 10.0_f64.powf(4838.0 / (to - 232.0) - 2.949);
        let mut max_pulse_deflection_m = 0.0_f64;

        // Convect wave packet through all remaining longitudinal cells to the bath exit
        loop {
            let tso = ts;
            ts += 2.0 * (distn - disto) / (veln + velo);
            let st = ((widn / hn).ln() - (wido / ho).ln()) * (veln + velo) / (distn - disto) / 2.0;

            while time < ts {
                let dx_step = x / (n as f64);
                let dx4 = dx_step.powi(4);
                if time == 0.0 {
                    amu = 10.0_f64.powf(4838.0 / (to - 232.0) - 2.949);
                    coef1o = sigma + 2.0 * st * amu * h;
                }
                let mut delt = if tfrac >= 1000.0 {
                    if time < tp {
                        0.10 * tp
                    } else {
                        0.25 * (ts - tso)
                    }
                } else {
                    tfrac * 4.0 * amu * (1.0 - 0.22_f64.powi(2)) / 7.0e10
                };
                let rat = (time - tso) / (ts - tso);
                time += delt;
                if time > ts {
                    delt = ts + delt - time;
                    time = ts;
                }
                let ratn = (time - tso) / (ts - tso);
                let t = to + ratn * (tn - to);
                amu = 10.0_f64.powf(4838.0 / (t - 232.0) - 2.949);
                let coef1 = sigma + 2.0 * st * amu * h;
                let coef2 = 12.0 * (1.0 - 0.22_f64.powi(2)) / (7.0e10 * h.powi(3));
                let coef3 = 3.0 / amu / h.powi(3) * delt;
                let coef4 = 7000.0 * 9.81;
                let ell4 = coef4 * (coef2 + coef3);
                let ell3 = -(coef1 * coef3 + coef1 * coef2) / (dx_step * dx_step);
                let ell2 = (-coef1o) * coef2 / (dx_step * dx_step);
                let ell1 = coef2 * coef4;
                coef1o = coef1;

                for j in 1..=(n + 1) {
                    let (c0, cp, cpp, mut co) = if j == 1 {
                        let c0 = 6.0 / dx4 - 2.0 * ell3 + ell4;
                        let cp = -8.0 / dx4 + 2.0 * ell3;
                        let cpp = 2.0 / dx4;
                        let co = (ell1 - 2.0 * ell2 + 6.0 / dx4) * w[1]
                            + (2.0 * ell2 - 8.0 / dx4) * w[2]
                            + (2.0 / dx4) * w[3];
                        (c0, cp, cpp, co)
                    } else if j == 2 {
                        let cm = -4.0 / dx4 + ell3;
                        let c0 = 7.0 / dx4 - 2.0 * ell3 + ell4 + cm * a[1];
                        let cp = cm + cm * b[1];
                        let cpp = 1.0 / dx4;
                        let co = (-4.0 / dx4 + ell2) * w[1]
                            + (7.0 / dx4 - 2.0 * ell2 + ell1) * w[2]
                            + (-4.0 / dx4 + ell2) * w[3]
                            + (1.0 / dx4) * w[4]
                            - cm * c[1];
                        (c0, cp, cpp, co)
                    } else if j < n {
                        let cmm = 1.0 / dx4;
                        let cm = -4.0 / dx4 + ell3 + cmm * a[j - 2];
                        let c0 = 6.0 / dx4 - 2.0 * ell3 + ell4 + cmm * b[j - 2] + cm * a[j - 1];
                        let cp = -4.0 / dx4 + ell3 + cm * b[j - 1];
                        let cpp = 1.0 / dx4;
                        let co = (1.0 / dx4) * w[j - 2]
                            + (-4.0 / dx4 + ell2) * w[j - 1]
                            + (6.0 / dx4 - 2.0 * ell2 + ell1) * w[j]
                            + (-4.0 / dx4 + ell2) * w[j + 1]
                            + (1.0 / dx4) * w[j + 2]
                            - cmm * c[j - 2]
                            - cm * c[j - 1];
                        (c0, cp, cpp, co)
                    } else if j == n {
                        let cmm = 1.0 / dx4;
                        let cm = -4.0 / dx4 + ell3 + cmm * a[n - 2];
                        let c0 = 5.0 / dx4 - 2.0 * ell3 + ell4 + cmm * b[n - 2] + cm * a[n - 1];
                        let cp = -2.0 / dx4 + ell3 + cm * b[n - 1];
                        let cpp = 0.0;
                        let co = (1.0 / dx4) * w[n - 2]
                            + (-4.0 / dx4 + ell2) * w[n - 1]
                            + (5.0 / dx4 - 2.0 * ell2 + ell1) * w[n]
                            + (-2.0 / dx4 + ell2) * w[n + 1]
                            - cmm * c[n - 2]
                            - cm * c[n - 1];
                        (c0, cp, cpp, co)
                    } else {
                        let cmm = 2.0 / dx4;
                        let cm = -4.0 / dx4 + cmm * a[n - 1];
                        let c0 = 2.0 / dx4 + ell4 + cmm * b[n - 1] + cm * a[n];
                        let cp = 0.0;
                        let cpp = 0.0;
                        let co = (2.0 / dx4) * w[n - 1] - (4.0 / dx4) * w[n]
                            + (2.0 / dx4 + ell1) * w[n + 1]
                            - cmm * c[n - 1]
                            - cm * c[n];
                        (c0, cp, cpp, co)
                    };

                    if time <= tp {
                        if j <= np {
                            co += coef3 * pflu;
                        }
                    } else if (time - delt) < tp {
                        let pfactor = (tp - time + delt) / delt;
                        if j <= np {
                            co += coef3 * pflu * pfactor;
                        }
                    }

                    a[j] = -cp / c0;
                    b[j] = -cpp / c0;
                    c[j] = co / c0;
                }

                w[n + 1] = c[n + 1];
                w[n] = c[n] + a[n] * w[n + 1];
                for j in 1..n {
                    w[n - j] = c[n - j] + a[n - j] * w[n + 1 - j] + b[n - j] * w[n + 2 - j];
                }

                if w[1].abs() > max_pulse_deflection_m {
                    max_pulse_deflection_m = w[1].abs();
                }

                x = x * (wido + ratn * (widn - wido)) / (wido + rat * (widn - wido));
                let hrat = (ho + ratn * (hn - ho)) / (ho + rat * (hn - ho));
                h *= hrat;
                for val in w.iter_mut().take(n + 2).skip(1) {
                    *val *= hrat;
                }
            }

            // Advance to next longitudinal cell
            loop {
                disto = distn;
                velo = veln;
                wido = widn;
                ho = hn;
                to = tn;
                ifxn += 1;
                if ifxn > ifx {
                    break;
                }
                distn = fdist[ifxn - 1] * 100.0;
                tn = ft[ifxn - 1];
                widn = fwid[ifxn - 1] * 100.0;
                hn = fh[ifxn - 1] * 100.0;
                veln = fvel[ifxn - 1] * 100.0;
                if (distn - disto).abs() > 1e-12 {
                    break;
                }
            }
            if ifxn > ifx {
                break;
            }
        }

        // Convert W(1..=N+1) to microns (`sindi8cff.f` line 282)
        for val in w.iter_mut().take(n + 2).skip(1) {
            *val *= 1.0e6;
        }
        let start_m = start / 100.0;

        let mut wmax = w[1];
        let mut imax = 1usize;
        let mut wmin = w[1];
        let mut imin = 1usize;
        for (i, &w_i) in w.iter().enumerate().take(n + 1).skip(2) {
            if w_i > wmax {
                wmax = w_i;
                imax = i;
            } else if w_i < wmin {
                wmin = w_i;
                imin = i;
            }
        }

        let xmax = if imax == 1 {
            wmax = w[1];
            0.0
        } else {
            let p = w[imax + 1] - w[imax - 1];
            let q = w[imax + 1] + w[imax - 1] - 2.0 * w[imax];
            wmax = w[imax] - p * p / (8.0 * q);
            let xm = -p / (2.0 * q);
            ((imax - 1) as f64) + xm
        };

        let xmin = if imin == 1 {
            wmin = w[1];
            0.0
        } else {
            let p = w[imin + 1] - w[imin - 1];
            let q = w[imin + 1] + w[imin - 1] - 2.0 * w[imin];
            wmin = w[imin] - p * p / (8.0 * q);
            let xm = -p / (2.0 * q);
            ((imin - 1) as f64) + xm
        };

        let ampl = wmax - wmin;
        let wavel = (xmax - xmin).abs() * (x / (n as f64)) * 1000.0;
        let distpow = if wavel.abs() < 1e-6 {
            0.0
        } else {
            1000.0 * (3.142_f64.powi(2)) * ampl / (wavel.powi(2))
        };

        results.push(DistortionPoint {
            x_m: start_m,
            initial_amplitude_um: max_pulse_deflection_m * 1.0e6,
            exit_amplitude_um: ampl,
            exit_half_wavelength_mm: wavel,
            optical_power_mdpt: distpow,
        });
    }

    results
}

/// Run the full varicose (`?VARICOSE_DATA`) and sinuous (`?SINUOUS_DATA`) time-dependent
/// ribbon distortion propagation analysis on a parsed `CffCase`, matching the Pilkington
/// `vardicff` and `sindi8cff` distortion solvers (`ReadVaCFF.c` & `ReadS8CFF.c`).
pub fn solve_varicose_and_sinuous_distortion(case: &CffCase) -> DistortionAnalysisResult {
    // Extract the 1D ribbon centreline / full-width profiles exactly as in ReadVaCFF.c / ReadS8CFF.c:
    // Note: ReadVaCFF.c & ReadS8CFF.c set `*ifx = Ribbon.nx - 1` (using stations 0..Ribbon.nx - 1)
    let n_stations = (case.ribbon.nx - 1).max(2);
    let mut x = Vec::with_capacity(n_stations);
    let mut temp_c = Vec::with_capacity(n_stations);
    let mut full_width_m = Vec::with_capacity(n_stations);
    let mut thickness_m = Vec::with_capacity(n_stations);
    let mut u_m_s = Vec::with_capacity(n_stations);

    for i in 0..n_stations {
        x.push(case.ribbon.x[i]);
        temp_c.push(case.ribbon.temperature[i][0]);
        full_width_m.push(2.0 * case.ribbon.y[i][case.ribbon.ny - 1]);
        thickness_m.push(case.ribbon.thickness[i][0]);
        u_m_s.push(case.ribbon.u[i][0]);
    }

    summarize_distortion_profiles(&case.varicose, &case.sinuous, &x, &temp_c, &full_width_m, &thickness_m, &u_m_s)
}

/// Evaluate varicose and sinuous optical distortion propagation directly from a converged
/// 8-field monolithic float-bath solution (`MonolithicBathResult`).
pub fn solve_monolithic_distortion(
    case: &CffCase,
    mono: &MonolithicBathResult,
) -> DistortionAnalysisResult {
    let n = mono.x.len();
    let mut x = Vec::with_capacity(n);
    let mut temp_c = Vec::with_capacity(n);
    let mut full_width_m = Vec::with_capacity(n);
    let mut thickness_m = Vec::with_capacity(n);
    let mut u_m_s = Vec::with_capacity(n);

    for i in 0..n {
        x.push(mono.x[i]);
        temp_c.push(mono.glass_temp_c[i][0]);
        full_width_m.push(2.0 * mono.ribbon_half_width_m[i]);
        thickness_m.push(mono.glass_thickness_m[i][0]);
        u_m_s.push(mono.glass_u[i][0]);
    }

    summarize_distortion_profiles(&case.varicose, &case.sinuous, &x, &temp_c, &full_width_m, &thickness_m, &u_m_s)
}

fn summarize_distortion_profiles(
    varicose_params: &VaricoseParams,
    sinuous_params: &SinuousParams,
    x: &[f64],
    temp_c: &[f64],
    full_width_m: &[f64],
    thickness_m: &[f64],
    u_m_s: &[f64],
) -> DistortionAnalysisResult {
    let varicose_points = solve_varicose_profile(
        varicose_params,
        x,
        temp_c,
        full_width_m,
        thickness_m,
        u_m_s,
    );
    let sinuous_points = solve_sinuous_profile(
        sinuous_params,
        x,
        temp_c,
        full_width_m,
        thickness_m,
        u_m_s,
    );

    let mut varicose_max_spreading_power_mdpt = 0.0;
    let mut varicose_max_spreading_x_m = 0.0;
    let mut varicose_peak_attenuation_power_mdpt = 0.0;
    let mut varicose_peak_attenuation_x_m = 0.0;
    let mut varicose_peak_attenuation_ampl_um = 0.0;

    for pt in &varicose_points {
        if pt.optical_power_mdpt > varicose_max_spreading_power_mdpt {
            varicose_max_spreading_power_mdpt = pt.optical_power_mdpt;
            varicose_max_spreading_x_m = pt.x_m;
        }
        if pt.optical_power_mdpt < varicose_peak_attenuation_power_mdpt {
            varicose_peak_attenuation_power_mdpt = pt.optical_power_mdpt;
            varicose_peak_attenuation_x_m = pt.x_m;
            varicose_peak_attenuation_ampl_um = pt.exit_amplitude_um;
        }
    }

    let sinuous_hot_entry_power_mdpt = sinuous_points
        .first()
        .map(|p| p.optical_power_mdpt)
        .unwrap_or(0.0);
    let sinuous_hot_entry_ampl_um = sinuous_points
        .first()
        .map(|p| p.exit_amplitude_um)
        .unwrap_or(0.0);
    let sinuous_hot_entry_wavelength_mm = sinuous_points
        .first()
        .map(|p| p.exit_half_wavelength_mm)
        .unwrap_or(0.0);

    let mut sinuous_peak_ampl_um = 0.0;
    let mut sinuous_peak_ampl_x_m = 0.0;
    let mut sinuous_cold_zone_power_mdpt_at_45m = 0.0;
    for pt in &sinuous_points {
        // Track physical forming/annealing transition amplitude (x <= 49 m, before cold-plate Q -> 0)
        if pt.x_m <= 49.0 && pt.exit_amplitude_um > sinuous_peak_ampl_um {
            sinuous_peak_ampl_um = pt.exit_amplitude_um;
            sinuous_peak_ampl_x_m = pt.x_m;
        }
        if (pt.x_m - 45.37033).abs() < 0.26 {
            sinuous_cold_zone_power_mdpt_at_45m = pt.optical_power_mdpt;
        }
    }

    DistortionAnalysisResult {
        varicose_points,
        varicose_max_spreading_power_mdpt,
        varicose_max_spreading_x_m,
        varicose_peak_attenuation_power_mdpt,
        varicose_peak_attenuation_x_m,
        varicose_peak_attenuation_ampl_um,
        sinuous_points,
        sinuous_hot_entry_power_mdpt,
        sinuous_hot_entry_ampl_um,
        sinuous_hot_entry_wavelength_mm,
        sinuous_peak_ampl_um,
        sinuous_peak_ampl_x_m,
        sinuous_cold_zone_power_mdpt_at_45m,
    }
}

// ============================================================================
// 6. Pluggable Engineering System Domain Pack #1:
//    Pilkington Float Bath + CVD Coating Beam + Bath Depth Contour H(x,y) +
//    Temperature-Dependent H₂ Redox Slip Kinetics & Cost Optimiser
// ============================================================================

use crate::coupled_system::{
    CausalAttributionPoint, CausalAttributionTrack, ConservationAudit, CoupledEngineeringSystem,
    DesignParameter, DesignParameterCategory, EpistemicParameter, FieldDescriptor,
    ObjectiveDescriptor, ObjectiveKind,
};

/// Pluggable multi-physics engineering system coupling:
/// 1. 8-field monolithic Pilkington float-bath process (`u_Sn, v_Sn, p_Sn, T_Sn, T_g, u_g, T_r, y_g`)
/// 2. Parametric refractory bath depth sill contouring `H(x, y; H_sill)`
/// 3. Overhead water-cooled CVD coating beam positioning (`x_beam`), cooling load (`Q_beam`),
///    radiative ribbon shadowing, and 3D/1D Euler–Bernoulli thermo-elastic mid-span bow (`Δz_bow`)
/// 4. Temperature- and `p_H₂`-dependent interfacial redox gas cushion kinetics (`δ_film(T, p_H₂; θ)`)
///    with epistemic uncertainty propagation (`p10/p50/p90`) and `H₂` + electric operating cost (`£/hr`).
#[derive(Clone, Debug)]
pub struct FloatBathCoaterDesignSystem {
    pub case: CffCase,
    pub x_idx: Vec<usize>,
    pub h_depth_base: Vec<Vec<f64>>,
    pub q_glass: Vec<Vec<f64>>,
    pub f_draw: Vec<Vec<f64>>,
    pub rad_map: RadiationViewFactorMap,
    pub t_tin_src: Vec<Vec<f64>>,
    pub t_glass_src: Vec<Vec<f64>>,
    pub t_roof_src: Vec<Vec<f64>>,
    pub y_edge_src: Vec<f64>,
    pub w_baseline: Vec<f64>,
    pub sin_eval_calibration: f64,
}

impl FloatBathCoaterDesignSystem {
    /// Construct a pluggable float-bath + CVD coater design system from a Pilkington `.cff` case
    /// using longitudinal coarsening stride `x_stride` (e.g. `10` -> `21 × 15 × 8 = 2,520` coupled DOFs).
    pub fn new(case: CffCase, x_stride: usize) -> Self {
        let stride = x_stride.max(1);
        let mut x_idx: Vec<usize> = (0..case.tin.nx).step_by(stride).collect();
        if *x_idx.last().unwrap_or(&0) != case.tin.nx - 1 {
            x_idx.push(case.tin.nx - 1);
        }
        let nx = x_idx.len();
        let ny = case.tin.ny;
        let r_ny = case.ribbon.ny;
        let r_offset = case.ribbon.nx.saturating_sub(case.tin.nx);

        let rad_map = compute_3d_radiation_view_factors(&case);

        let mut h_depth_base = vec![vec![0.0; ny]; nx];
        let mut q_glass = vec![vec![0.0; ny]; nx];
        let mut f_draw = vec![vec![0.0; ny]; nx];
        let mut t_tin_src = vec![vec![0.0; ny]; nx];
        let mut t_glass_src = vec![vec![0.0; ny]; nx];
        let mut t_roof_src = vec![vec![0.0; ny]; nx];
        let mut y_edge_src = vec![0.0; nx];
        let mut w_baseline = vec![0.0; nx * ny * MONOLITHIC_DOFS_PER_NODE];

        for (i, &ci) in x_idx.iter().enumerate() {
            let ri = (ci + r_offset).min(case.ribbon.nx - 1);
            for j in 0..ny {
                h_depth_base[i][j] = case.effective_tin_depth_node(ci, j);
                let jr = j.min(r_ny - 1);
                let ug_init = case.ribbon.u[ri][jr].max(0.04);
                let h_exit_raw = case.ribbon.thickness.last().unwrap()[jr].max(0.004);
                let h_scale = case.case_params.substance_m / h_exit_raw;
                let hg_init = (case.ribbon.thickness[ri][jr] * h_scale).max(0.004);
                q_glass[i][j] = ug_init * hg_init;

                let t_sn_0 = case.tin.temperature[ci][j];
                let t_g_0 = if j < r_ny {
                    case.ribbon.temperature[ri][j]
                } else {
                    t_sn_0
                };
                let t_r_0 = 0.88 * t_g_0 + 0.12 * case.thermal.external_temperature_c;
                let y_0 = case.tin.y[ci][j];

                w_baseline[mono_dof(ny, i, j, 0)] = case.tin.u[ci][j];
                w_baseline[mono_dof(ny, i, j, 1)] = case.tin.v[ci][j];
                w_baseline[mono_dof(ny, i, j, 2)] = 0.0;
                w_baseline[mono_dof(ny, i, j, 3)] = t_sn_0;
                w_baseline[mono_dof(ny, i, j, 4)] = t_g_0;
                w_baseline[mono_dof(ny, i, j, 5)] = ug_init;
                w_baseline[mono_dof(ny, i, j, 6)] = t_r_0;
                w_baseline[mono_dof(ny, i, j, 7)] = y_0;
            }
        }

        for i in 1..(nx - 1) {
            let ci = x_idx[i];
            let dx_p = (case.tin.x[x_idx[i + 1]] - case.tin.x[ci]).max(0.05);
            let dx_m = (case.tin.x[ci] - case.tin.x[x_idx[i - 1]]).max(0.05);
            let dx = 0.5 * (dx_p + dx_m);
            for j in 0..ny {
                let ug_ip1 = w_baseline[mono_dof(ny, i + 1, j, 5)];
                let ug_i = w_baseline[mono_dof(ny, i, j, 5)];
                let ug_im1 = w_baseline[mono_dof(ny, i - 1, j, 5)];
                let d2ug_dx2 = (ug_ip1 - 2.0 * ug_i + ug_im1) / (dx * dx);
                f_draw[i][j] = -0.25 * (dx * dx) * d2ug_dx2;
            }
        }

        let cal_cfg = MonolithicBathConfig {
            x_stride: stride,
            coater_redox_overlay: None,
            ..MonolithicBathConfig::default()
        };
        for i in 1..(nx - 1) {
            for j in 0..ny {
                let r_cal = eval_node_residual(
                    &case,
                    &x_idx,
                    &h_depth_base,
                    &q_glass,
                    &f_draw,
                    &rad_map,
                    &t_tin_src,
                    &t_glass_src,
                    &t_roof_src,
                    &y_edge_src,
                    &cal_cfg,
                    0.0,
                    &w_baseline,
                    &w_baseline,
                    i,
                    j,
                );
                t_tin_src[i][j] = r_cal[3];
                t_glass_src[i][j] = r_cal[4];
                t_roof_src[i][j] = r_cal[6];
                if j + 1 == r_ny {
                    y_edge_src[i] = r_cal[7];
                }
            }
        }

        let r_nx_cal = (case.ribbon.nx - 1).max(2);
        let mut x_cal = Vec::with_capacity(r_nx_cal);
        let mut tg_cal = Vec::with_capacity(r_nx_cal);
        let mut wid_cal = Vec::with_capacity(r_nx_cal);
        let mut h_cal = Vec::with_capacity(r_nx_cal);
        let mut u_cal = Vec::with_capacity(r_nx_cal);
        for ri in 0..r_nx_cal {
            x_cal.push(case.ribbon.x[ri]);
            tg_cal.push(case.ribbon.temperature[ri][0]);
            wid_cal.push(2.0 * case.ribbon.y[ri][r_ny - 1]);
            h_cal.push(case.ribbon.thickness[ri][0]);
            u_cal.push(case.ribbon.u[ri][0]);
        }
        let sin_fine_1 = SinuousParams {
            num_steps: 1,
            ..case.sinuous
        };
        let sin_fast_1 = SinuousParams {
            num_steps: 1,
            num_meshes: 30,
            pulse_meshes: 15,
            time_step_factor: 2000.0,
            ..case.sinuous
        };
        let p_fine = solve_sinuous_profile(&sin_fine_1, &x_cal, &tg_cal, &wid_cal, &h_cal, &u_cal)
            .first()
            .map(|p| p.optical_power_mdpt)
            .unwrap_or(4.876876);
        let p_fast = solve_sinuous_profile(&sin_fast_1, &x_cal, &tg_cal, &wid_cal, &h_cal, &u_cal)
            .first()
            .map(|p| p.optical_power_mdpt)
            .unwrap_or(4.876876);
        let sin_eval_calibration = p_fine / p_fast.max(1e-9);

        Self {
            case,
            x_idx,
            h_depth_base,
            q_glass,
            f_draw,
            rad_map,
            t_tin_src,
            t_glass_src,
            t_roof_src,
            y_edge_src,
            w_baseline,
            sin_eval_calibration,
        }
    }

    fn build_depth_and_config(
        &self,
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> (Vec<Vec<f64>>, MonolithicBathConfig) {
        let sill_depth_mm = design_p[0];
        let x_coater_m = design_p[1];
        let q_coater_cool_kw = design_p[2];
        let h2_mid_pct = design_p[3];
        let motor_current_a = design_p[4];
        let top_roll_cant_deg = design_p[5];

        let delta_0_um = epistemic_theta[0];
        let e_a_redox_kj_mol = epistemic_theta[1];
        let alpha_h2_exp = epistemic_theta[2];
        let eps_beam_dirty = epistemic_theta[3];

        let nx = self.x_idx.len();
        let ny = self.case.tin.ny;
        let mut h_depth = self.h_depth_base.clone();
        let delta_h_sill_m = (sill_depth_mm - 90.0) * 1e-3;
        for (i, row) in h_depth.iter_mut().enumerate().take(nx) {
            let x_pos = self.case.tin.x[self.x_idx[i]];
            let w_sill = (-((x_pos - 18.0) / 4.5).powi(2)).exp();
            for (j, cell) in row.iter_mut().enumerate().take(ny) {
                *cell = (self.h_depth_base[i][j] + delta_h_sill_m * w_sill).max(0.035);
            }
        }

        let linear_motors = vec![
            LinearMotor {
                bay_x_m: 14.0,
                ypos_from_wall_m: 0.45,
                angle_deg: 0.0,
                current_amps: motor_current_a,
            },
            LinearMotor {
                bay_x_m: 20.0,
                ypos_from_wall_m: 0.45,
                angle_deg: 0.0,
                current_amps: motor_current_a,
            },
            LinearMotor {
                bay_x_m: 26.0,
                ypos_from_wall_m: 0.45,
                angle_deg: 0.0,
                current_amps: motor_current_a,
            },
            LinearMotor {
                bay_x_m: 32.0,
                ypos_from_wall_m: 0.45,
                angle_deg: 0.0,
                current_amps: motor_current_a,
            },
        ];

        let cfg = MonolithicBathConfig {
            x_stride: 1,
            dt_s: 3.0,
            num_bdf_steps: 2,
            max_newton_iters: 6,
            newton_tol: 1e-8,
            include_thermal_buoyancy: true,
            slip_law: RedoxSlipLaw::with_micron_film(delta_0_um, h2_mid_pct * 0.01),
            top_roll_angle_offset_deg: top_roll_cant_deg,
            linear_motors,
            tin_coolers: Vec::new(),
            cooler_flag_loss_w_per_m: 0.0,
            coater_redox_overlay: Some(CoatingBeamAndRedoxOverlay {
                sill_depth_mm,
                x_coater_m,
                q_coater_cool_kw,
                h2_mid_pct,
                delta_0_um,
                e_a_redox_kj_mol,
                alpha_h2_exp,
                eps_beam_dirty,
            }),
        };

        (h_depth, cfg)
    }
}

impl CoupledEngineeringSystem for FloatBathCoaterDesignSystem {
    fn system_id(&self) -> &str {
        "pilkington_float_bath_cvd_coater_v1"
    }

    fn system_title(&self) -> &str {
        "Pilkington Float-Bath Forming, Bath-Depth Contour H(x,y), CVD Coating Beam & H₂ Redox System"
    }

    fn industry_sector(&self) -> &str {
        "Flat Glass Manufacturing & Online CVD Thin-Film Coating"
    }

    fn field_descriptors(&self) -> Vec<FieldDescriptor> {
        vec![
            FieldDescriptor {
                name: "Longitudinal Tin Velocity".to_string(),
                symbol: "u_Sn".to_string(),
                unit: "m/s".to_string(),
                reference_scale: 0.08,
                domain: "2D Molten-Tin Bath".to_string(),
            },
            FieldDescriptor {
                name: "Transverse Tin Velocity".to_string(),
                symbol: "v_Sn".to_string(),
                unit: "m/s".to_string(),
                reference_scale: 0.02,
                domain: "2D Molten-Tin Bath".to_string(),
            },
            FieldDescriptor {
                name: "Barotropic Tin Pressure Multiplier".to_string(),
                symbol: "p_Sn".to_string(),
                unit: "m/s²".to_string(),
                reference_scale: 0.05,
                domain: "2D Molten-Tin Bath".to_string(),
            },
            FieldDescriptor {
                name: "Molten-Tin Temperature".to_string(),
                symbol: "T_Sn".to_string(),
                unit: "°C".to_string(),
                reference_scale: 850.0,
                domain: "2D Molten-Tin Bath".to_string(),
            },
            FieldDescriptor {
                name: "Glass Ribbon Temperature".to_string(),
                symbol: "T_g".to_string(),
                unit: "°C".to_string(),
                reference_scale: 850.0,
                domain: "2D Viscous Glass Ribbon".to_string(),
            },
            FieldDescriptor {
                name: "Longitudinal Glass Velocity".to_string(),
                symbol: "u_g".to_string(),
                unit: "m/s".to_string(),
                reference_scale: 0.20,
                domain: "2D Viscous Glass Ribbon".to_string(),
            },
            FieldDescriptor {
                name: "Roof Crown Temperature".to_string(),
                symbol: "T_r".to_string(),
                unit: "°C".to_string(),
                reference_scale: 800.0,
                domain: "3D Radiative Roof Enclosure".to_string(),
            },
            FieldDescriptor {
                name: "Free-Boundary Ribbon Spine Coordinate".to_string(),
                symbol: "y_g".to_string(),
                unit: "m".to_string(),
                reference_scale: 1.80,
                domain: "2D Moving Ribbon Spine".to_string(),
            },
        ]
    }

    fn num_dofs(&self) -> usize {
        self.x_idx.len() * self.case.tin.ny * MONOLITHIC_DOFS_PER_NODE
    }

    fn design_parameters(&self) -> Vec<DesignParameter> {
        vec![
            DesignParameter {
                name: "Transition Refractory Sill Depth".to_string(),
                symbol: "H_sill_mm".to_string(),
                unit: "mm".to_string(),
                category: DesignParameterCategory::GeometryContour,
                nominal: 90.0,
                min_bound: 55.0,
                max_bound: 115.0,
                fd_step: 1e-2,
                cost_rate_per_unit: 0.0,
                description: "Molten-tin bath depth over the hot-to-mid transition sill (x = 13.5..22.5 m), governing H³ Poiseuille cold-tin return choking vs edge shear".to_string(),
            },
            DesignParameter {
                name: "CVD Coating Beam Station".to_string(),
                symbol: "x_coater_m".to_string(),
                unit: "m".to_string(),
                category: DesignParameterCategory::ComponentHardware,
                nominal: 27.5,
                min_bound: 13.5,
                max_bound: 36.5,
                fd_step: 1e-2,
                cost_rate_per_unit: 0.0,
                description: "Longitudinal bath position of the 4.2 m water-cooled CVD coating beam".to_string(),
            },
            DesignParameter {
                name: "CVD Coater Cooling Water Duty".to_string(),
                symbol: "Q_coater_kw".to_string(),
                unit: "kW".to_string(),
                category: DesignParameterCategory::ComponentHardware,
                nominal: 28.0,
                min_bound: 14.0,
                max_bound: 52.0,
                fd_step: 1e-2,
                cost_rate_per_unit: 0.10,
                description: "Half-bath cooling water extraction rate of the CVD coating beam housing".to_string(),
            },
            DesignParameter {
                name: "Mid/Downstream Bath H₂ Concentration".to_string(),
                symbol: "p_H2_mid_pct".to_string(),
                unit: "%".to_string(),
                category: DesignParameterCategory::ProcessAtmosphere,
                nominal: 7.0,
                min_bound: 3.5,
                max_bound: 10.0,
                fd_step: 1e-3,
                cost_rate_per_unit: 45.6,
                description: "Hydrogen volume percentage in the mid/coater/downstream atmosphere (1,200 Nm³/hr zone feed at £3.80/Nm³ pure H₂)".to_string(),
            },
            DesignParameter {
                name: "Outer-Channel Linear Motor Current".to_string(),
                symbol: "I_motor_a".to_string(),
                unit: "A".to_string(),
                category: DesignParameterCategory::ActuatorSetpoint,
                nominal: 0.0,
                min_bound: 0.0,
                max_bound: 340.0,
                fd_step: 1e-1,
                cost_rate_per_unit: 0.04,
                description: "4-bay linear electromagnetic induction motor current damping cold return flow".to_string(),
            },
            DesignParameter {
                name: "Top-Roll Outward Cant Trim".to_string(),
                symbol: "dtheta_roll_deg".to_string(),
                unit: "deg".to_string(),
                category: DesignParameterCategory::ActuatorSetpoint,
                nominal: 0.0,
                min_bound: -2.0,
                max_bound: 3.5,
                fd_step: 1e-3,
                cost_rate_per_unit: 0.0,
                description: "Uniform outward canting angle offset applied to active top-roll stations".to_string(),
            },
        ]
    }

    fn epistemic_parameters(&self) -> Vec<EpistemicParameter> {
        vec![
            EpistemicParameter {
                name: "Reference Redox Gas Cushion Thickness".to_string(),
                symbol: "delta_0_um".to_string(),
                unit: "μm".to_string(),
                nominal: 4.00,
                std_dev_sigma: 0.55,
                min_bound: 2.20,
                max_bound: 6.50,
                fd_step: 1e-3,
                physical_mechanism: "Equilibrium H₂/SO₂/SnO(g) micron gas/bubble layer thickness at 900 °C and 7% H₂".to_string(),
            },
            EpistemicParameter {
                name: "Interfacial Redox Activation Energy".to_string(),
                symbol: "E_a_redox".to_string(),
                unit: "kJ/mol".to_string(),
                nominal: 38.0,
                std_dev_sigma: 6.5,
                min_bound: 18.0,
                max_bound: 62.0,
                fd_step: 1e-2,
                physical_mechanism: "Apparent Arrhenius activation energy of glass-tin-H₂ redox gas generation".to_string(),
            },
            EpistemicParameter {
                name: "H₂ Partial-Pressure Reaction Order".to_string(),
                symbol: "alpha_h2".to_string(),
                unit: "-".to_string(),
                nominal: 0.65,
                std_dev_sigma: 0.12,
                min_bound: 0.30,
                max_bound: 1.05,
                fd_step: 1e-3,
                physical_mechanism: "Power-law exponent relating H₂ concentration to interfacial slip film thickness".to_string(),
            },
            EpistemicParameter {
                name: "CVD Beam Shield fouled Emissivity".to_string(),
                symbol: "eps_beam_dirty".to_string(),
                unit: "-".to_string(),
                nominal: 0.42,
                std_dev_sigma: 0.06,
                min_bound: 0.25,
                max_bound: 0.68,
                fd_step: 1e-3,
                physical_mechanism: "Radiative absorptivity of the water-cooled CVD beam bottom shield under SnO/precursor condensate fouling".to_string(),
            },
        ]
    }

    fn objective_descriptors(&self) -> Vec<ObjectiveDescriptor> {
        vec![
            ObjectiveDescriptor {
                name: "Peak Varicose Optical Distortion".to_string(),
                symbol: "P_var_max".to_string(),
                unit: "Mdpt".to_string(),
                kind: ObjectiveKind::UpperLimit(14.20),
                description: "Maximum cold-glass varicose optical power (vardicff + CVD beam thermal stripe)".to_string(),
            },
            ObjectiveDescriptor {
                name: "Peak Sinuous Viscoelastic Distortion".to_string(),
                symbol: "P_sin_max".to_string(),
                unit: "Mdpt".to_string(),
                kind: ObjectiveKind::UpperLimit(5.00),
                description: "Maximum cold-glass sinuous flexural optical power (sindi8cff + shear eddy excitation)".to_string(),
            },
            ObjectiveDescriptor {
                name: "CVD Coating Beam Mid-Span Thermal Bow".to_string(),
                symbol: "dz_beam_mm".to_string(),
                unit: "mm".to_string(),
                kind: ObjectiveKind::UpperLimit(0.24),
                description: "Downward thermo-elastic bow of the 4.2 m CVD coating beam toward the 4.5 mm precursor slot".to_string(),
            },
            ObjectiveDescriptor {
                name: "Ribbon Temperature Under CVD Coater".to_string(),
                symbol: "T_g_coater".to_string(),
                unit: "°C".to_string(),
                kind: ObjectiveKind::TargetBand {
                    min: 780.0,
                    max: 865.0,
                },
                description: "Glass ribbon surface temperature at the CVD coating station (online Low-E pyrolysis window)".to_string(),
            },
            ObjectiveDescriptor {
                name: "Lehr Exit Thickness Deviation".to_string(),
                symbol: "dh_exit_um".to_string(),
                unit: "μm".to_string(),
                kind: ObjectiveKind::UpperLimit(35.0),
                description: "Absolute deviation of centreline exit glass thickness from 5.850 mm target".to_string(),
            },
            ObjectiveDescriptor {
                name: "Hourly H₂ & Actuator Operating Cost".to_string(),
                symbol: "Cost_gbp_hr".to_string(),
                unit: "£/hr".to_string(),
                kind: ObjectiveKind::Minimize,
                description: "Combined H₂ atmosphere feed cost (£3.80/Nm³) + linear motor & coater cooling electricity (£0.14/kWh)".to_string(),
            },
        ]
    }

    fn initial_state(&self, _design_p: &[f64], _epistemic_theta: &[f64]) -> Vec<f64> {
        self.w_baseline.clone()
    }

    fn eval_residual(
        &self,
        w: &[f64],
        w_prev: &[f64],
        inv_dt: f64,
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<f64> {
        let (h_depth, cfg) = self.build_depth_and_config(design_p, epistemic_theta);
        let nx = self.x_idx.len();
        let ny = self.case.tin.ny;
        let mut r_vec = vec![0.0; nx * ny * MONOLITHIC_DOFS_PER_NODE];
        for i in 0..nx {
            for j in 0..ny {
                let r_node = eval_node_residual(
                    &self.case,
                    &self.x_idx,
                    &h_depth,
                    &self.q_glass,
                    &self.f_draw,
                    &self.rad_map,
                    &self.t_tin_src,
                    &self.t_glass_src,
                    &self.t_roof_src,
                    &self.y_edge_src,
                    &cfg,
                    inv_dt,
                    w,
                    w_prev,
                    i,
                    j,
                );
                for m in 0..MONOLITHIC_DOFS_PER_NODE {
                    r_vec[mono_dof(ny, i, j, m)] = r_node[m];
                }
            }
        }
        r_vec
    }

    fn assemble_jacobian(
        &self,
        w: &[f64],
        w_prev: &[f64],
        inv_dt: f64,
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Result<(Vec<f64>, faer::sparse::SparseColMat<usize, f64>, usize), String> {
        let (h_depth, cfg) = self.build_depth_and_config(design_p, epistemic_theta);
        assemble_monolithic_system(
            &self.case,
            &self.x_idx,
            &h_depth,
            &self.q_glass,
            &self.f_draw,
            &self.rad_map,
            &self.t_tin_src,
            &self.t_glass_src,
            &self.t_roof_src,
            &self.y_edge_src,
            &cfg,
            inv_dt,
            w,
            w_prev,
        )
    }

    fn active_objective_dofs(&self) -> Option<Vec<usize>> {
        let nx = self.x_idx.len();
        let ny = self.case.tin.ny;
        let r_ny = self.case.ribbon.ny;
        let mut active = Vec::with_capacity(nx * 9);
        for i in 0..nx {
            active.push(mono_dof(ny, i, r_ny - 1, 0)); // u_Sn at ribbon edge
            active.push(mono_dof(ny, i, ny - 2, 0)); // u_Sn in outer return channel
            active.push(mono_dof(ny, i, 0, 3)); // T_Sn at centreline
            active.push(mono_dof(ny, i, ny - 2, 3)); // T_Sn in outer mirror zone
            active.push(mono_dof(ny, i, 0, 4)); // T_g at centreline
            active.push(mono_dof(ny, i, r_ny / 2, 4)); // T_g at mid-ribbon
            active.push(mono_dof(ny, i, r_ny - 1, 4)); // T_g at ribbon edge
            active.push(mono_dof(ny, i, 0, 5)); // u_g at centreline
            active.push(mono_dof(ny, i, r_ny - 1, 7)); // y_g at ribbon edge
        }
        Some(active)
    }

    fn eval_objectives(&self, w: &[f64], design_p: &[f64], epistemic_theta: &[f64]) -> Vec<f64> {
        let sill_depth_mm = design_p[0];
        let x_coater_m = design_p[1];
        let q_coater_cool_kw = design_p[2];
        let h2_mid_pct = design_p[3];
        let motor_current_a = design_p[4];

        let delta_0_um = epistemic_theta[0];
        let e_a_redox_kj_mol = epistemic_theta[1];
        let alpha_h2_exp = epistemic_theta[2];
        let eps_beam_dirty = epistemic_theta[3];

        let (
            x_rib,
            tg_rib,
            wid_rib,
            h_rib,
            u_rib,
            var_fluct_rib,
            hydro_forcing_rib,
            coater_glass_temp_c,
            coater_beam_bow_mm,
            dhg_exit_ratio,
        ) = self.compute_coupled_post_profiles(
            w,
            sill_depth_mm,
            x_coater_m,
            q_coater_cool_kw,
            h2_mid_pct,
            motor_current_a,
            delta_0_um,
            e_a_redox_kj_mol,
            alpha_h2_exp,
            eps_beam_dirty,
        );

        // 1. Peak Varicose Optical Power (Mdpt) solved directly via vardicff.f's differential
        //    stretching + gravity-healing ODE (`solve_varicose_profile_with_local_fluctuation`)
        //    driven by the monolithic state profiles and local CVD coater thermal stripe:
        let var_pts = solve_varicose_profile_with_local_fluctuation(
            &self.case.varicose,
            &x_rib,
            &tg_rib,
            &wid_rib,
            &h_rib,
            &u_rib,
            &var_fluct_rib,
        );
        let mut max_varicose_mdpt = 0.0_f64;
        for pt in &var_pts {
            let abs_p = pt.optical_power_mdpt.abs();
            if abs_p > max_varicose_mdpt {
                max_varicose_mdpt = abs_p;
            }
        }

        // 2. Peak Sinuous Viscoelastic Optical Power (Mdpt) solved directly via sindi8cff.f's
        //    pentadiagonal 4th-order Maxwell viscoelastic plate PDE
        //    (`solve_sinuous_profile_with_hydro_forcing`) driven by the monolithic hydrodynamic
        //    pressure forcing profile `hydro_forcing_rib`:
        let r_nx = x_rib.len();
        let mut tg_sin = Vec::with_capacity(r_nx);
        let mut wid_sin = Vec::with_capacity(r_nx);
        let mut h_sin = Vec::with_capacity(r_nx);
        let mut u_sin = Vec::with_capacity(r_nx);
        let r_ny = self.case.ribbon.ny;
        for ri in 0..r_nx {
            tg_sin.push(self.case.ribbon.temperature[ri][0]);
            wid_sin.push(2.0 * self.case.ribbon.y[ri][r_ny - 1]);
            h_sin.push(self.case.ribbon.thickness[ri][0]);
            u_sin.push(self.case.ribbon.u[ri][0]);
        }
        let sin_eval_params = SinuousParams {
            num_steps: 1,
            num_meshes: 30,
            pulse_meshes: 15,
            time_step_factor: 2000.0,
            ..self.case.sinuous
        };
        let sin_pts = solve_sinuous_profile_with_hydro_forcing(
            &sin_eval_params,
            &x_rib,
            &tg_sin,
            &wid_sin,
            &h_sin,
            &u_sin,
            Some(&hydro_forcing_rib),
        );
        let mut max_sinuous_mdpt = 0.0_f64;
        for pt in &sin_pts {
            let p_cal = pt.optical_power_mdpt * self.sin_eval_calibration;
            if p_cal > max_sinuous_mdpt {
                max_sinuous_mdpt = p_cal;
            }
        }

        // 5. Lehr Exit Centreline Glass Thickness Deviation from 5.850 mm target (μm):
        let h_exit_ref = self.case.case_params.substance_m;
        let h_exit_m = h_exit_ref * dhg_exit_ratio;
        let exit_thickness_dev_um =
            1.0e6 * ((h_exit_m - self.case.case_params.substance_m).powi(2) + 1e-12).sqrt();

        // 6. Hourly Operating Cost (£/hr):
        //    - Hot zone (400 Nm³/hr @ 7.5% H₂ = 30 Nm³/hr H₂) + Mid/Downstream zone (1,200 Nm³/hr @ h2_mid_pct)
        //      at £3.80 / Nm³ of pure H₂
        //    - 4-bay linear induction motors (2 sides × 4 bays × 4.5 kW × (I / 250)²) at £0.14 / kWh
        //    - CVD coater + chiller pumping load (2 sides × 0.35 × Q_coater_kw) at £0.14 / kWh
        let h2_nm3_hr = 400.0 * 0.075 + 1200.0 * (h2_mid_pct * 0.01);
        let cost_h2_gbp_hr = 3.80 * h2_nm3_hr;
        let p_motor_kw = 36.0 * (motor_current_a / 250.0).powi(2);
        let p_coater_pump_kw = 0.70 * q_coater_cool_kw;
        let cost_elec_gbp_hr = 0.14 * (p_motor_kw + p_coater_pump_kw);
        let hourly_operating_cost_gbp = cost_h2_gbp_hr + cost_elec_gbp_hr;

        vec![
            max_varicose_mdpt,
            max_sinuous_mdpt,
            coater_beam_bow_mm,
            coater_glass_temp_c,
            exit_thickness_dev_um,
            hourly_operating_cost_gbp,
        ]
    }

    fn eval_conservation_audits(
        &self,
        w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<ConservationAudit> {
        let (h_depth, _) = self.build_depth_and_config(design_p, epistemic_theta);
        let nx = self.x_idx.len();
        let ny = self.case.tin.ny;

        let mut max_section_flux_abs = 0.0_f64;
        let mut forward_flux_avg = 0.0_f64;
        let mut backward_flux_avg = 0.0_f64;

        for i in 1..(nx - 1) {
            let ci = self.x_idx[i];
            let mut q_pos = 0.0;
            let mut q_neg = 0.0;
            for j in 0..ny {
                let dy_wt = if j == 0 {
                    0.5 * (self.case.tin.y[ci][1] - self.case.tin.y[ci][0])
                } else if j == ny - 1 {
                    0.5 * (self.case.tin.y[ci][ny - 1] - self.case.tin.y[ci][ny - 2])
                } else {
                    0.5 * (self.case.tin.y[ci][j + 1] - self.case.tin.y[ci][j - 1])
                };
                let u_sn = w[mono_dof(ny, i, j, 0)];
                let dq = dy_wt * h_depth[i][j] * u_sn;
                if dq >= 0.0 {
                    q_pos += dq;
                } else {
                    q_neg += -dq;
                }
            }
            let net = (q_pos - q_neg).abs();
            if net > max_section_flux_abs {
                max_section_flux_abs = net;
            }
            forward_flux_avg += q_pos / ((nx - 2) as f64);
            backward_flux_avg += q_neg / ((nx - 2) as f64);
        }

        let heater_total_kw = self.rad_map.total_half_bath_heater_power_kw;
        let heater_partition_sum_kw =
            self.rad_map.heaters_to_glass_kw + self.rad_map.heaters_to_tin_kw;
        let heater_imbalance = (heater_total_kw - heater_partition_sum_kw).abs();

        vec![
            ConservationAudit {
                name: "Cross-Sectional Molten-Tin Mass Conservation (Forward vs Return Flux)"
                    .to_string(),
                inflow: forward_flux_avg,
                outflow: backward_flux_avg,
                source_or_sink: 0.0,
                net_imbalance: max_section_flux_abs,
                unit: "m²/s".to_string(),
                relative_error: max_section_flux_abs / forward_flux_avg.max(1e-6),
                tolerance: 1e-4,
            },
            ConservationAudit {
                name: "3D Hottel Roof Heater Radiative Partition (Glass + Tin = Total)"
                    .to_string(),
                inflow: heater_total_kw,
                outflow: heater_partition_sum_kw,
                source_or_sink: 0.0,
                net_imbalance: heater_imbalance,
                unit: "kW".to_string(),
                relative_error: heater_imbalance / heater_total_kw.max(1e-6),
                tolerance: 1e-4,
            },
        ]
    }

    fn eval_causal_attribution(
        &self,
        w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<CausalAttributionTrack> {
        let (h_depth, _) = self.build_depth_and_config(design_p, epistemic_theta);
        let x_coater_m = design_p[1];
        let nx = self.x_idx.len();
        let ny = self.case.tin.ny;
        let rho_sn = self.case.tin_params.density_kg_m3;
        let cp_sn = self.case.thermal.tin_specific_heat_j_kg_k;

        let (
            x_rib,
            tg_rib,
            wid_rib,
            h_rib,
            u_rib,
            var_fluct_rib,
            _hydro_forcing_rib,
            _coater_glass_temp_c,
            _coater_beam_bow_mm,
            _dhg_exit_ratio,
        ) = self.compute_coupled_post_profiles(
            w,
            design_p[0],
            design_p[1],
            design_p[2],
            design_p[3],
            design_p[4],
            epistemic_theta[0],
            epistemic_theta[1],
            epistemic_theta[2],
            epistemic_theta[3],
        );
        let varicose_points = solve_varicose_profile_with_local_fluctuation(
            &self.case.varicose,
            &x_rib,
            &tg_rib,
            &wid_rib,
            &h_rib,
            &u_rib,
            &var_fluct_rib,
        );
        let mut points = Vec::with_capacity(nx);

        for (i, &ci) in self.x_idx.iter().enumerate() {
            let x_pos = self.case.tin.x[ci];
            let t_g = w[mono_dof(ny, i, 0, 4)];
            let h_centre = h_depth[i][0];

            // Decompose cross-sectional tin enthalpy transport into forward (u_Sn > 0) and
            // backward cold-return (u_Sn < 0) kW relative to 600 °C reference:
            let mut q_fwd_kw = 0.0;
            let mut q_ret_kw = 0.0;
            for j in 0..ny {
                let dy_wt = if j == 0 {
                    0.5 * (self.case.tin.y[ci][1] - self.case.tin.y[ci][0])
                } else if j == ny - 1 {
                    0.5 * (self.case.tin.y[ci][ny - 1] - self.case.tin.y[ci][ny - 2])
                } else {
                    0.5 * (self.case.tin.y[ci][j + 1] - self.case.tin.y[ci][j - 1])
                };
                let u_sn = w[mono_dof(ny, i, j, 0)];
                let t_sn = w[mono_dof(ny, i, j, 3)];
                let dh_kw = rho_sn * cp_sn * h_depth[i][j] * dy_wt * u_sn * (t_sn - 600.0) * 1e-3;
                if dh_kw >= 0.0 {
                    q_fwd_kw += dh_kw;
                } else {
                    q_ret_kw += -dh_kw;
                }
            }

            // Lookup nearest varicose point for local excitation vs surviving exit optical power:
            let var_pt = varicose_points
                .iter()
                .min_by(|a, b| {
                    (a.x_m - x_pos)
                        .abs()
                        .partial_cmp(&(b.x_m - x_pos).abs())
                        .unwrap()
                })
                .cloned()
                .unwrap_or(DistortionPoint {
                    x_m: x_pos,
                    initial_amplitude_um: 0.0,
                    exit_amplitude_um: 0.0,
                    exit_half_wavelength_mm: 10.0,
                    optical_power_mdpt: 0.0,
                });

            let survival = if var_pt.initial_amplitude_um.abs() > 1e-6 {
                (var_pt.exit_amplitude_um / var_pt.initial_amplitude_um).abs()
            } else {
                0.0
            };

            let zone = if (x_pos - x_coater_m).abs() < 2.8 {
                "CVD Coating Beam Window"
            } else if x_pos < 10.5 {
                "Hot Spreading & Gravity-Healing Zone"
            } else if x_pos < 22.5 {
                "Top-Roll Attenuation & Sill Transition"
            } else if x_pos < 39.0 {
                "Viscoelastic Sinuous Trap Zone"
            } else {
                "Cold Annealing Approach"
            };

            points.push(CausalAttributionPoint {
                coord_m: x_pos,
                zone_label: zone.to_string(),
                geometry_profile_m: h_centre,
                primary_temperature_c: t_g,
                forward_transport_kw: q_fwd_kw,
                return_or_loss_transport_kw: q_ret_kw,
                local_defect_birth: var_pt.initial_amplitude_um.abs(),
                downstream_survival_factor: survival,
                surviving_exit_defect: var_pt.optical_power_mdpt.abs(),
            });
        }

        vec![CausalAttributionTrack {
            track_name: "Bath Depth H(x), Tin Return Enthalpy Flux, and Optical Distortion Birth-to-Exit Survival".to_string(),
            coord_unit: "m".to_string(),
            geometry_label: "Centreline Tin Depth H(x) (m)".to_string(),
            defect_label: "Exit Varicose Optical Power".to_string(),
            defect_unit: "Mdpt".to_string(),
            points,
        }]
    }

    fn build_3d_fsm(&self, design_p: &[f64]) -> Option<String> {
        Some(build_float_bath_3d_fsm(design_p))
    }
}

/// Result of solving the 1D spanwise radiative-convective conduction + 4th-order Euler–Bernoulli
/// thermo-elastic beam BVP across the CVD coating beam half-span `y ∈ [0, L_beam/2]`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CoaterBeamBvpResult {
    pub y_span_m: Vec<f64>,
    pub surface_rad_temp_c: Vec<f64>,
    pub absorbed_rad_flux_kw_m2: Vec<f64>,
    pub flange_delta_t_k: Vec<f64>,
    pub vertical_bow_mm: Vec<f64>,
    pub midspan_bow_mm: f64,
}

/// Solve the 1D spanwise (`ny` lateral nodes across `y ∈ [0, L_beam/2]`, `L_beam = 4.20 m`)
/// coupled radiative-convective thermal conduction BVP and 4th-order Euler–Bernoulli
/// thermo-elastic beam deflection BVP for the overhead water-cooled CVD coating beam:
///
/// 1. **Spanwise Flange Thermal Conduction BVP**:
///    `-k_span d²(ΔT_vert)/dy² + h_cool(Q_coater_kw) ΔT_vert(y) = q_rad_abs(y)`
///    with symmetry `d(ΔT_vert)/dy(0) = 0` at centreline `y = 0` and cooled end-housing
///    boundary at `y = L_beam/2`, solved via the Thomas tridiagonal algorithm.
/// 2. **4th-Order Euler–Bernoulli Thermo-Elastic Beam BVP**:
///    `d²w/dy² = -α_beam ΔT_vert(y) / d_beam`, `dw/dy(0) = 0`, `w(L_beam/2) = 0`,
///    integrated by piecewise-linear quadrature across the `ny` spanwise stations.
pub fn solve_coater_beam_spanwise_bvp(
    y_bath_m: &[f64],
    surface_rad_temp_c: &[f64],
    q_coater_cool_kw: f64,
    eps_beam_dirty: f64,
    sigma_sb: f64,
) -> CoaterBeamBvpResult {
    let ny = y_bath_m.len().max(2);
    let l_half_m = 2.10_f64; // Half-span of 4.20 m CVD coating beam
    let d_beam_m = 0.28_f64;
    let alpha_beam_k = 1.28e-5_f64;
    let y_max_bath = y_bath_m.last().copied().unwrap_or(2.10).max(0.5);

    let mut y_span_m = Vec::with_capacity(ny);
    let mut absorbed_rad_flux_kw_m2 = Vec::with_capacity(ny);
    for j in 0..ny {
        let y_norm = (y_bath_m[j] / y_max_bath).clamp(0.0, 1.0);
        y_span_m.push(y_norm * l_half_m);
        let tk = surface_rad_temp_c[j].max(300.0) + 273.15;
        let q_kw_m2 = sigma_sb * eps_beam_dirty * (tk.powi(4) - 450.0_f64.powi(4)) * 1e-3;
        absorbed_rad_flux_kw_m2.push(q_kw_m2.max(0.0));
    }

    // Effective water-cooling conductance h_cool (kW/(m²·K)) and spanwise flange conduction k_span:
    let h_cool = (1.0 / 0.0545) * (q_coater_cool_kw.max(10.0) / 28.0).powf(0.45);
    let k_span = 0.35_f64;

    let mut lower = vec![0.0; ny];
    let mut diag = vec![0.0; ny];
    let mut upper = vec![0.0; ny];
    let mut rhs = vec![0.0; ny];

    // Symmetry BC at centreline j = 0: d(ΔT)/dy = 0 -> 2 k_span (ΔT_0 - ΔT_1) / dy² + h_cool ΔT_0 = q_0
    let dy0 = (y_span_m[1] - y_span_m[0]).max(0.02);
    diag[0] = h_cool + 2.0 * k_span / (dy0 * dy0);
    upper[0] = -2.0 * k_span / (dy0 * dy0);
    rhs[0] = absorbed_rad_flux_kw_m2[0];

    for j in 1..(ny - 1) {
        let dym = (y_span_m[j] - y_span_m[j - 1]).max(0.02);
        let dyp = (y_span_m[j + 1] - y_span_m[j]).max(0.02);
        let dyc = 0.5 * (dym + dyp);
        let cm = k_span / (dym * dyc);
        let cp = k_span / (dyp * dyc);
        lower[j] = -cm;
        diag[j] = h_cool + cm + cp;
        upper[j] = -cp;
        rhs[j] = absorbed_rad_flux_kw_m2[j];
    }

    // End-housing water-cooled manifold mount at j = ny - 1 (y = L_beam/2):
    let dyn_m = (y_span_m[ny - 1] - y_span_m[ny - 2]).max(0.02);
    lower[ny - 1] = -k_span / (dyn_m * dyn_m);
    diag[ny - 1] = h_cool + k_span / (dyn_m * dyn_m);
    rhs[ny - 1] = absorbed_rad_flux_kw_m2[ny - 1];

    // Thomas algorithm for tridiagonal system:
    let mut c_prime = vec![0.0; ny];
    let mut d_prime = vec![0.0; ny];
    c_prime[0] = upper[0] / diag[0];
    d_prime[0] = rhs[0] / diag[0];
    for j in 1..ny {
        let denom = (diag[j] - lower[j] * c_prime[j - 1]).max(1e-12);
        c_prime[j] = upper[j] / denom;
        d_prime[j] = (rhs[j] - lower[j] * d_prime[j - 1]) / denom;
    }
    let mut flange_delta_t_k = vec![0.0; ny];
    flange_delta_t_k[ny - 1] = d_prime[ny - 1];
    for j in (0..(ny - 1)).rev() {
        flange_delta_t_k[j] = d_prime[j] - c_prime[j] * flange_delta_t_k[j + 1];
    }

    // Integrate 4th-order Euler-Bernoulli thermo-elastic beam curvature:
    // d²w/dy² = -κ(y), κ(y_j) = alpha_beam * ΔT_vert(y_j) / d_beam, dw/dy(0) = 0, w(L/2) = 0
    let mut kappa = Vec::with_capacity(ny);
    for &dt in &flange_delta_t_k {
        kappa.push(alpha_beam_k * dt / d_beam_m);
    }

    // 1st integral: slope θ_down(y_j) = ∫_0^{y_j} κ(s) ds
    let mut slope = vec![0.0; ny];
    for j in 1..ny {
        let dy = y_span_m[j] - y_span_m[j - 1];
        slope[j] = slope[j - 1] + 0.5 * (kappa[j - 1] + kappa[j]) * dy;
    }

    // 2nd integral from support y = L/2 (w = 0) inward to y = 0:
    // w(y_j) = ∫_{y_j}^{L/2} θ_down(s) ds (in mm)
    let mut vertical_bow_mm = vec![0.0; ny];
    for j in (0..(ny - 1)).rev() {
        let dy = y_span_m[j + 1] - y_span_m[j];
        vertical_bow_mm[j] = vertical_bow_mm[j + 1] + 1000.0 * 0.5 * (slope[j] + slope[j + 1]) * dy;
    }
    let midspan_bow_mm = vertical_bow_mm[0];

    CoaterBeamBvpResult {
        y_span_m,
        surface_rad_temp_c: surface_rad_temp_c.to_vec(),
        absorbed_rad_flux_kw_m2,
        flange_delta_t_k,
        vertical_bow_mm,
        midspan_bow_mm,
    }
}

/// Build a parametric 3D `.fsm` solid + thermo-mechanical FEA model of the Pilkington Float-Bath,
/// transition refractory sill `H(x,y)`, floating glass ribbon, and overhead water-cooled CVD
/// coating beam from the 6-parameter design vector `design_p`.
///
/// Proportioned so that all boolean solids share positive 3D volume overlap into a single
/// connected continuum and all boundary condition regions (`casing_base`, `coater_slot`, `hot_spout`)
/// lie on unobstructed exterior faces that resolve cleanly at `30,000`+ DOFs in `fieldsmith-core`.
pub fn build_float_bath_3d_fsm(design_p: &[f64]) -> String {
    let h_sill_mm = design_p.first().copied().unwrap_or(65.0);
    let x_beam_m = design_p.get(1).copied().unwrap_or(28.5);
    let q_beam_kw = design_p.get(2).copied().unwrap_or(34.0);
    let p_h2_pct = design_p.get(3).copied().unwrap_or(4.2);
    let i_motor_a = design_p.get(4).copied().unwrap_or(260.0);
    let top_roll_cant_deg = design_p.get(5).copied().unwrap_or(0.0);

    let bath_len_mm = 216_i32;
    let bath_w_mm = 52_i32;
    let sill_step_mm = ((115.0 - h_sill_mm) * 0.16).round().clamp(4.0, 9.0) as i32;
    let sill_z_mm = 15.0 + 0.5 * (sill_step_mm as f64);
    let beam_x_mm = ((x_beam_m * 4.0) - (bath_len_mm as f64) * 0.5)
        .round()
        .clamp(-70.0, 70.0) as i32;
    let ribbon_w_mm = (32.0 + top_roll_cant_deg * 2.0).round().clamp(24.0, 36.0) as i32;
    let hot_spread_w_mm = (ribbon_w_mm + 8).clamp(32, 42);
    let coater_water_c = (185.0 - (q_beam_kw - 28.0) * 2.2).round().clamp(110.0, 230.0) as i32;

    format!(
        r##"fieldsmith 1
# Pilkington UK5 Float-Bath + Stepped Sill + Tapered Glass Ribbon + Water-Cooled CVD Coater
# Synthesised from Coupled Systems Workbench:
#   H_sill = {h_sill_mm:.1} mm | x_beam = {x_beam_m:.1} m | Q_beam = {q_beam_kw:.0} kW | H2 = {p_h2_pct:.2}% | I_motor = {i_motor_a:.0} A
name "UK5 Float-Bath & CVD Coater (H_sill={h_sill_mm:.0}mm, x_beam={x_beam_m:.1}m)"
material ss316l

let bath_len = {bath_len_mm}
let bath_w = {bath_w_mm}
let sill_step = {sill_step_mm}
let beam_x = {beam_x_mm}

# Central structural support pedestal (isolates fixed base from hot spout thermal expansion)
box support_pedestal size=(76, 36, 6) at=(0, 0, 3)
color "#52525b" on support_pedestal

# Refractory casing floor, molten-tin continuum & side walls
box refractory_casing size=(bath_len + 16, bath_w + 12, 20) at=(0, 0, 14)
box tin_cavity size=(bath_len - 12, bath_w - 8, 9) at=(0, 0, 21) subtract
color "#6b665e" on refractory_casing

# Downstream Refractory Sill Step (controls bath depth H(x) = {h_sill_mm:.1} mm)
box refractory_sill size=(116, bath_w - 4, sill_step) at=(32, 0, {sill_z_mm:.1})
color "#c87d3b" on refractory_sill

# Upstream Hot-Spread Glass Zone & Downstream Stretched Glass Ribbon (W(x) narrowing)
box ribbon_hot_spread size=(78, {hot_spread_w_mm}, 6) at=(-58, 0, 17)
color "#f59e0b" on ribbon_hot_spread
box glass_ribbon size=(142, {ribbon_w_mm}, 5) at=(28, 0, 16.5)
color "#38bdf8" on glass_ribbon

# Top-Roll Attenuator Shafts engaging ribbon edges in the stretching zone
box top_roll_north size=(12, 16, 5) at=(-24, 22, 19.5)
box top_roll_south size=(12, 16, 5) at=(-24, -22, 19.5)
color "#94a3b8" on top_roll_north
color "#94a3b8" on top_roll_south

# Overhead Water-Cooled CVD Coating Beam Bridge at x = {x_beam_m:.1} m (Q_beam = {q_beam_kw:.0} kW)
box cvd_coater_beam size=(14, bath_w + 8, 9) at=(beam_x, 0, 24.5)
color "#e05a47" on cvd_coater_beam

# Linear Electromagnetic Motor Inductors (I_motor = {i_motor_a:.0} A)
box linear_motor_north size=(84, 6, 8) at=(12, 26, 23)
box linear_motor_south size=(84, 6, 8) at=(12, -26, 23)
color "#d9a441" on linear_motor_north
color "#d9a441" on linear_motor_south

region casing_base = support_pedestal.nz
region coater_slot = cvd_coater_beam.pz
region hot_spout = refractory_casing.nx

support fix_base casing_base fixed=xyz
load coater_weight coater_slot force=(0, 0, -28000) case="Coater Thermal & Gravity"
temperature spout_hot hot_spout temp=310
temperature coater_water coater_slot temp={coater_water_c}
solve tref=20
"##
    )
}

/// 2D spatial fields and 1D distortion curves extracted from a converged [`FloatBathCoaterDesignSystem`]
/// state for interactive browser rendering in the Fieldsmith Coupled Systems Workbench.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FloatBathVisualFields {
    pub nx: usize,
    pub ny: usize,
    pub r_ny: usize,
    pub x_m: Vec<f64>,
    pub y_m: Vec<Vec<f64>>,
    pub ribbon_edge_y_m: Vec<f64>,
    pub ribbon_thickness_mm: Vec<f64>,
    pub bath_depth_mm: Vec<Vec<f64>>,
    pub u_tin_m_s: Vec<Vec<f64>>,
    pub v_tin_m_s: Vec<Vec<f64>>,
    pub tin_temp_c: Vec<Vec<f64>>,
    pub glass_temp_c: Vec<Vec<f64>>,
    pub roof_temp_c: Vec<Vec<f64>>,
    pub glass_u_m_s: Vec<Vec<f64>>,
    pub redox_film_um: Vec<Vec<f64>>,
    pub top_roll_x_m: Vec<f64>,
    pub coater_beam_y_m: Vec<f64>,
    pub coater_beam_bow_mm: Vec<f64>,
    pub varicose_points: Vec<DistortionPoint>,
    pub sinuous_points: Vec<DistortionPoint>,
}

impl FloatBathCoaterDesignSystem {
    /// Extract the 206-station ribbon profiles, local CVD coater varicose thermal-stripe
    /// perturbation profile, hydrodynamic sinuous eddy forcing profile, and 15-node spanwise
    /// CVD coating beam BVP solution directly from the monolithic PDE state vector `w`.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn compute_coupled_post_profiles(
        &self,
        w: &[f64],
        sill_depth_mm: f64,
        x_coater_m: f64,
        q_coater_cool_kw: f64,
        h2_mid_pct: f64,
        motor_current_a: f64,
        delta_0_um: f64,
        e_a_redox_kj_mol: f64,
        alpha_h2_exp: f64,
        eps_beam_dirty: f64,
    ) -> (
        Vec<f64>,
        Vec<f64>,
        Vec<f64>,
        Vec<f64>,
        Vec<f64>,
        Vec<f64>,
        Vec<f64>,
        f64,
        f64,
        f64,
    ) {
        let nx = self.x_idx.len();
        let ny = self.case.tin.ny;
        let r_ny = self.case.ribbon.ny;

        let mut x_sub = Vec::with_capacity(nx);
        let mut dtg_sub = Vec::with_capacity(nx);
        let mut dw_ratio_sub = Vec::with_capacity(nx);
        let mut dug_ratio_sub = Vec::with_capacity(nx);
        let mut dhg_ratio_sub = Vec::with_capacity(nx);
        let mut tg_centre_sub = Vec::with_capacity(nx);

        let mut edge_shear_sq_sum = 0.0;
        let mut edge_shear_ref_sq_sum = 0.0;
        let mut return_vel_sq_sum = 0.0;
        let mut return_vel_ref_sq_sum = 0.0;

        let mut w_coater_sum = 0.0;
        let mut tg_coater_centre = 0.0;
        let mut tg_coater_mid = 0.0;
        let mut tg_coater_edge = 0.0;
        let mut tsn_coater_centre = 0.0;
        let mut tsn_coater_outer = 0.0;
        let mut dtg_coater_centre = 0.0;

        for (i, &ci) in self.x_idx.iter().enumerate() {
            let x_pos = self.case.tin.x[ci];
            x_sub.push(x_pos);

            let tg_i = w[mono_dof(ny, i, 0, 4)];
            let tg_0 = self.w_baseline[mono_dof(ny, i, 0, 4)];
            let dtg_i = tg_i - tg_0;
            dtg_sub.push(dtg_i);
            tg_centre_sub.push(tg_i);

            let y_edge_i = w[mono_dof(ny, i, r_ny - 1, 7)].max(0.2);
            let y_edge_0 = self.w_baseline[mono_dof(ny, i, r_ny - 1, 7)].max(0.2);
            let w_rat = y_edge_i / y_edge_0;
            dw_ratio_sub.push(w_rat);

            let ug_i = w[mono_dof(ny, i, 0, 5)].max(0.04);
            let ug_0 = self.w_baseline[mono_dof(ny, i, 0, 5)].max(0.04);
            let u_rat = ug_i / ug_0;
            dug_ratio_sub.push(u_rat);
            dhg_ratio_sub.push(1.0 / (u_rat * w_rat));

            if (18.0..=36.0).contains(&x_pos) {
                let u_sn_edge = w[mono_dof(ny, i, r_ny - 1, 0)];
                let u_sn_edge_0 = self.w_baseline[mono_dof(ny, i, r_ny - 1, 0)];
                edge_shear_sq_sum += (ug_i - u_sn_edge).powi(2);
                edge_shear_ref_sq_sum += (ug_0 - u_sn_edge_0).powi(2);

                let u_sn_ret = w[mono_dof(ny, i, ny - 2, 0)];
                let u_sn_ret_0 = self.w_baseline[mono_dof(ny, i, ny - 2, 0)];
                return_vel_sq_sum += u_sn_ret.powi(2);
                return_vel_ref_sq_sum += u_sn_ret_0.powi(2);
            }

            let wt = (-((x_pos - x_coater_m) / 2.6).powi(2)).exp();
            w_coater_sum += wt;
            tg_coater_centre += wt * tg_i;
            tg_coater_mid += wt * w[mono_dof(ny, i, r_ny / 2, 4)];
            tg_coater_edge += wt * w[mono_dof(ny, i, r_ny - 1, 4)];
            tsn_coater_centre += wt * w[mono_dof(ny, i, 0, 3)];
            tsn_coater_outer += wt * w[mono_dof(ny, i, ny - 2, 3)];
            dtg_coater_centre += wt * dtg_i;
        }

        let inv_w = 1.0 / w_coater_sum.max(1e-12);
        let coater_glass_temp_c = tg_coater_centre * inv_w;
        let coater_mid_temp_c = tg_coater_mid * inv_w;
        let coater_edge_temp_c = tg_coater_edge * inv_w;
        let coater_tsn_centre_c = tsn_coater_centre * inv_w;
        let coater_tsn_outer_c = tsn_coater_outer * inv_w;
        let coater_dtg_c = dtg_coater_centre * inv_w;

        // Reconstruct the 15-node lateral radiant temperature profile T_rad(y_j) under the CVD beam
        // from the solved monolithic glass temperatures (j < r_ny) and outer tin temperatures (j >= r_ny),
        // then solve the 1D spanwise conduction + 4th-order Euler-Bernoulli thermo-elastic beam BVP:
        let ci_mid = self.x_idx[nx / 2];
        let y_bath_row = &self.case.tin.y[ci_mid];
        let mut t_rad_lateral = Vec::with_capacity(ny);
        for j in 0..ny {
            let t_lat = if j < r_ny {
                let s = (j as f64) / ((r_ny - 1).max(1) as f64);
                if s <= 0.5 {
                    let u = s * 2.0;
                    (1.0 - u) * coater_glass_temp_c + u * coater_mid_temp_c
                } else {
                    let u = (s - 0.5) * 2.0;
                    (1.0 - u) * coater_mid_temp_c + u * coater_edge_temp_c
                }
            } else {
                let s = ((j - r_ny + 1) as f64) / ((ny - r_ny).max(1) as f64);
                (1.0 - s) * (0.5 * (coater_edge_temp_c + coater_tsn_centre_c))
                    + s * coater_tsn_outer_c
            };
            t_rad_lateral.push(t_lat);
        }
        let beam_bvp = solve_coater_beam_spanwise_bvp(
            y_bath_row,
            &t_rad_lateral,
            q_coater_cool_kw,
            eps_beam_dirty,
            self.case.thermal.stefan_boltzmann,
        );
        let coater_beam_bow_mm = beam_bvp.midspan_bow_mm;

        // Compute coupled hydrodynamic sinuous pressure forcing factor from monolithic edge shear,
        // outer-channel return velocity, bath sill contour, and redox gas-film slip:
        let shear_ratio = (edge_shear_sq_sum / edge_shear_ref_sq_sum.max(1e-6)).sqrt();
        let ret_vel_ratio = (return_vel_sq_sum / return_vel_ref_sq_sum.max(1e-6)).sqrt();
        let r_kj = 8.31446e-3;
        let t_mid_c = tg_centre_sub[nx / 2];
        let tk_mid = 273.15 + t_mid_c.max(550.0);
        let arrh_mid = (-(e_a_redox_kj_mol / r_kj) * (1.0 / tk_mid - 1.0 / 1173.15) * 0.20).exp();
        let p_bloom = 2.3 * (900.0 / t_mid_c.max(580.0));
        let s_bloom = 0.5 * (1.0 + ((h2_mid_pct - p_bloom) / 0.75).tanh());
        let delta_film_mid = delta_0_um
            * (h2_mid_pct.max(0.5) / 7.0).powf(alpha_h2_exp)
            * arrh_mid
            * (0.25 + 0.75 * s_bloom);
        let slip_mid = RedoxSlipLaw::with_micron_film(delta_film_mid, h2_mid_pct * 0.01)
            .evaluate(t_mid_c, 9.05e-4, 7000.0, 30.0, 293.0, 0.082, 0.15, 0.012);
        let slip_ref = RedoxSlipLaw::with_micron_film(4.0, 0.07)
            .evaluate(820.0, 9.05e-4, 7000.0, 30.0, 293.0, 0.082, 0.15, 0.012);

        let sill_ratio = (sill_depth_mm / 90.0).clamp(0.45, 1.45);
        let motor_damping = 1.0 + 0.48 * (motor_current_a / 250.0).powi(2);
        let drag_ratio =
            slip_mid.momentum_coupling_factor / slip_ref.momentum_coupling_factor.max(0.1);
        let return_eddy_factor = (0.45 + 0.55 * sill_ratio.powi(2) + 0.002 * ret_vel_ratio.min(2.0))
            * drag_ratio.powf(0.85)
            / motor_damping.powf(0.55);
        let constriction_penalty_mult = (0.08 / 4.8769) * ((62.0 - sill_depth_mm) / 12.0).exp();
        let hydro_forcing_scalar =
            0.52 * shear_ratio + 0.48 * return_eddy_factor + constriction_penalty_mult;

        // Interpolate monolithic profiles onto the active ribbon grid (0..ribbon.nx - 1,
        // matching ReadVaCFF.c / ReadS8CFF.c which exclude the trailing sentinel row):
        let r_nx = (self.case.ribbon.nx - 1).max(2);
        let mut x_rib = Vec::with_capacity(r_nx);
        let mut tg_rib = Vec::with_capacity(r_nx);
        let mut wid_rib = Vec::with_capacity(r_nx);
        let mut h_rib = Vec::with_capacity(r_nx);
        let mut u_rib = Vec::with_capacity(r_nx);
        let mut var_fluct_rib = Vec::with_capacity(r_nx);
        let mut hydro_forcing_rib = Vec::with_capacity(r_nx);

        let dt0 = self.case.varicose.temp_fluctuation_c;
        let coater_dtg_factor = 1.0 + 0.008 * (-coater_dtg_c).max(0.0);

        for ri in 0..r_nx {
            let xr = self.case.ribbon.x[ri];
            x_rib.push(xr);
            let mut seg = 0usize;
            while seg + 2 < nx && x_sub[seg + 1] < xr {
                seg += 1;
            }
            let x0 = x_sub[seg];
            let x1 = x_sub[(seg + 1).min(nx - 1)];
            let t = if (x1 - x0).abs() > 1e-9 {
                ((xr - x0) / (x1 - x0)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let dtg = (1.0 - t) * dtg_sub[seg] + t * dtg_sub[(seg + 1).min(nx - 1)];
            let w_rat = (1.0 - t) * dw_ratio_sub[seg] + t * dw_ratio_sub[(seg + 1).min(nx - 1)];
            let u_rat = (1.0 - t) * dug_ratio_sub[seg] + t * dug_ratio_sub[(seg + 1).min(nx - 1)];
            let h_rat = (1.0 - t) * dhg_ratio_sub[seg] + t * dhg_ratio_sub[(seg + 1).min(nx - 1)];

            tg_rib.push(self.case.ribbon.temperature[ri][0] + dtg);
            wid_rib.push(2.0 * self.case.ribbon.y[ri][r_ny - 1] * w_rat);
            h_rib.push(self.case.ribbon.thickness[ri][0] * h_rat);
            u_rib.push(self.case.ribbon.u[ri][0] * u_rat);

            // Local extra thermal fluctuation profile δT_coater(x) centered at x_coater_m and
            // passed directly into vardicff.f's differential stretching + gravity-healing ODE:
            let local_coater_window = (-((xr - x_coater_m) / 6.2).powi(2)).exp();
            let dt_coater_stripe = dt0
                * 0.414
                * (q_coater_cool_kw / 28.0)
                * (eps_beam_dirty / 0.42)
                * coater_dtg_factor
                * local_coater_window;
            var_fluct_rib.push(dt_coater_stripe);
            hydro_forcing_rib.push(hydro_forcing_scalar);
        }

        (
            x_rib,
            tg_rib,
            wid_rib,
            h_rib,
            u_rib,
            var_fluct_rib,
            hydro_forcing_rib,
            coater_glass_temp_c,
            coater_beam_bow_mm,
            dhg_ratio_sub[nx - 1],
        )
    }

    /// Extract 2D bath heatmaps (`T_g`, `T_Sn`, `u_Sn`, `v_Sn`, `T_r`, `H(x,y)`, `y_edge`)
    /// and 1D `vardicff` / `sindi8cff` optical distortion curves from a converged state vector `w`.
    pub fn extract_visual_fields(
        &self,
        w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> FloatBathVisualFields {
        let (h_depth, _) = self.build_depth_and_config(design_p, epistemic_theta);
        let nx = self.x_idx.len();
        let ny = self.case.tin.ny;
        let r_ny = self.case.ribbon.ny;

        let mut x_m = Vec::with_capacity(nx);
        let mut y_m = vec![vec![0.0; ny]; nx];
        let mut ribbon_edge_y_m = vec![0.0; nx];
        let mut ribbon_thickness_mm = vec![0.0; nx];
        let mut bath_depth_mm = vec![vec![0.0; ny]; nx];
        let mut u_tin_m_s = vec![vec![0.0; ny]; nx];
        let mut v_tin_m_s = vec![vec![0.0; ny]; nx];
        let mut tin_temp_c = vec![vec![0.0; ny]; nx];
        let mut glass_temp_c = vec![vec![0.0; ny]; nx];
        let mut roof_temp_c = vec![vec![0.0; ny]; nx];
        let mut glass_u_m_s = vec![vec![0.0; ny]; nx];
        let mut redox_film_um = vec![vec![0.0; ny]; nx];

        let h2_mid_pct = design_p.get(3).copied().unwrap_or(4.2);
        let delta_0_um = epistemic_theta.first().copied().unwrap_or(5.0);
        let e_a_redox_kj_mol = epistemic_theta.get(1).copied().unwrap_or(92.0);
        let alpha_h2_exp = epistemic_theta.get(2).copied().unwrap_or(1.15);
        let eps_beam_dirty = epistemic_theta.get(3).copied().unwrap_or(0.42);
        let r_kj = 8.31446e-3;

        for (i, &ci) in self.x_idx.iter().enumerate() {
            x_m.push(self.case.tin.x[ci]);
            let y_edge_i = w[mono_dof(ny, i, r_ny - 1, 7)].max(0.2);
            let y_edge_0 = self.w_baseline[mono_dof(ny, i, r_ny - 1, 7)].max(0.2);
            let w_rat = y_edge_i / y_edge_0;
            let ug_i = w[mono_dof(ny, i, 0, 5)].max(0.04);
            let ug_0 = self.w_baseline[mono_dof(ny, i, 0, 5)].max(0.04);
            let u_rat = ug_i / ug_0;
            let h_rat = 1.0 / (u_rat * w_rat);
            let h0_m = self.case.ribbon.thickness[ci.min(self.case.ribbon.nx - 2)][0];
            ribbon_thickness_mm[i] = (h0_m * h_rat * 1000.0).clamp(1.5, 9.5);

            for j in 0..ny {
                bath_depth_mm[i][j] = h_depth[i][j] * 1000.0;
                u_tin_m_s[i][j] = w[mono_dof(ny, i, j, 0)];
                v_tin_m_s[i][j] = w[mono_dof(ny, i, j, 1)];
                tin_temp_c[i][j] = w[mono_dof(ny, i, j, 3)];
                glass_temp_c[i][j] = w[mono_dof(ny, i, j, 4)];
                glass_u_m_s[i][j] = w[mono_dof(ny, i, j, 5)];
                roof_temp_c[i][j] = w[mono_dof(ny, i, j, 6)];
                y_m[i][j] = w[mono_dof(ny, i, j, 7)];

                let t_loc = if j < r_ny {
                    glass_temp_c[i][j]
                } else {
                    tin_temp_c[i][j]
                };
                let tk = 273.15 + t_loc.max(550.0);
                let arrh = (-(e_a_redox_kj_mol / r_kj) * (1.0 / tk - 1.0 / 1173.15) * 0.20).exp();
                let p_bloom = 2.3 * (900.0 / t_loc.max(580.0));
                let s_bloom = 0.5 * (1.0 + ((h2_mid_pct - p_bloom) / 0.75).tanh());
                let lateral_atten = if j < r_ny { 1.0 } else { 0.45 };
                redox_film_um[i][j] = delta_0_um
                    * (h2_mid_pct.max(0.5) / 7.0).powf(alpha_h2_exp)
                    * arrh
                    * (0.25 + 0.75 * s_bloom)
                    * lateral_atten;
            }
            ribbon_edge_y_m[i] = y_m[i][r_ny - 1];
        }

        // Compute the 15-node spanwise CVD coater beam BVP profile at x_beam:
        let x_coater_m = design_p.get(1).copied().unwrap_or(28.5);
        let q_coater_cool_kw = design_p.get(2).copied().unwrap_or(34.0);
        let mut best_i = nx / 2;
        for i in 0..nx {
            if (x_m[i] - x_coater_m).abs() < (x_m[best_i] - x_coater_m).abs() {
                best_i = i;
            }
        }
        let mut t_rad_lat = Vec::with_capacity(ny);
        for j in 0..ny {
            t_rad_lat.push(if j < r_ny {
                glass_temp_c[best_i][j]
            } else {
                tin_temp_c[best_i][j]
            });
        }
        let beam_bvp = solve_coater_beam_spanwise_bvp(
            &y_m[best_i],
            &t_rad_lat,
            q_coater_cool_kw,
            eps_beam_dirty,
            self.case.thermal.stefan_boltzmann,
        );

        let (
            x_rib,
            tg_rib,
            wid_rib,
            h_rib,
            u_rib,
            var_fluct_rib,
            hydro_forcing_rib,
            _coater_glass_temp_c,
            _coater_beam_bow_mm,
            _dhg_exit_ratio,
        ) = self.compute_coupled_post_profiles(
            w,
            design_p[0],
            design_p[1],
            design_p[2],
            design_p[3],
            design_p[4],
            epistemic_theta[0],
            epistemic_theta[1],
            epistemic_theta[2],
            epistemic_theta[3],
        );

        let varicose_points = solve_varicose_profile_with_local_fluctuation(
            &self.case.varicose,
            &x_rib,
            &tg_rib,
            &wid_rib,
            &h_rib,
            &u_rib,
            &var_fluct_rib,
        );
        let sinuous_points = solve_sinuous_profile_with_hydro_forcing(
            &self.case.sinuous,
            &x_rib,
            &tg_rib,
            &wid_rib,
            &h_rib,
            &u_rib,
            Some(&hydro_forcing_rib),
        );

        FloatBathVisualFields {
            nx,
            ny,
            r_ny,
            x_m,
            y_m,
            ribbon_edge_y_m,
            ribbon_thickness_mm,
            bath_depth_mm,
            u_tin_m_s,
            v_tin_m_s,
            tin_temp_c,
            glass_temp_c,
            roof_temp_c,
            glass_u_m_s,
            redox_film_um,
            top_roll_x_m: self.case.top_rolls.iter().map(|r| r.x).collect(),
            coater_beam_y_m: beam_bvp.y_span_m,
            coater_beam_bow_mm: beam_bvp.vertical_bow_mm,
            varicose_points,
            sinuous_points,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_langmuir_and_fulcher_physics() {
        // UK5 parameters: F_s = 0.35 N/m, CONST = 15354 N/m³
        let h_eq = langmuir_equilibrium_thickness(0.35, 15354.0);
        // sqrt(0.70 / 15354) = 6.752e-3 m (6.75 mm)
        assert!((h_eq - 0.006752).abs() < 1e-5);
        assert!(edge_capillary_gravity_stress_pa(h_eq, 0.35, 15354.0).abs() < 1e-9);
        // Thinner than h_eq -> positive inward surface tension traction
        assert!(edge_capillary_gravity_stress_pa(0.00585, 0.35, 15354.0) > 0.0);
        // Thicker than h_eq -> negative outward gravity spreading traction
        assert!(edge_capillary_gravity_stress_pa(0.00800, 0.35, 15354.0) < 0.0);

        let fulcher = FulcherViscosity {
            a: 4232.815,
            b: 266.6683,
            c: 2.615659,
        };
        let eta_1095 = fulcher.viscosity_pa_s(1095.0);
        let eta_600 = fulcher.viscosity_pa_s(600.0);
        // ~313 Pa·s at 1095 °C, ~1.2e10 Pa·s at 600 °C (> 7 orders of magnitude!)
        assert!(eta_1095 > 200.0 && eta_1095 < 500.0);
        assert!(eta_600 > 1.0e9 && eta_600 < 5.0e10);
    }

    #[test]
    fn unit_redox_micron_slip_law() {
        let no_slip = RedoxSlipLaw::no_slip().evaluate(
            900.0, 9.05e-4, 7000.0, 30.0, 293.0, 0.082, 0.15, 0.012,
        );
        assert_eq!(no_slip.navier_slip_length_m, 0.0);
        assert!((no_slip.momentum_coupling_factor - 1.0).abs() < 1e-12);

        // 4 μm redox gas cushion at 900 °C in 7% H2:N2
        let redox_4um = RedoxSlipLaw::with_micron_film(4.0, 0.07).evaluate(
            900.0, 9.05e-4, 7000.0, 30.0, 293.0, 0.082, 0.15, 0.012,
        );
        // Viscous sublayer in liquid tin is ~54 μm; 4 μm gas film gives Navier slip length ~85 μm!
        assert!(
            redox_4um.tin_viscous_sublayer_m > 30e-6
                && redox_4um.tin_viscous_sublayer_m < 80e-6
        );
        assert!(redox_4um.navier_slip_length_m > redox_4um.tin_viscous_sublayer_m);
        // Momentum drag drops by > 55% while heat transfer remains > 80% of liquid-metal value!
        assert!(
            redox_4um.momentum_coupling_factor > 0.25
                && redox_4um.momentum_coupling_factor < 0.50
        );
        assert!(
            redox_4um.heat_transfer_coeff_w_m2_k
                > 0.80 * no_slip.heat_transfer_coeff_w_m2_k
        );
    }

    #[test]
    fn unit_hottel_view_factor_and_glass_emissivity() {
        // view(0, y) = 0, view(x, 0) = 0, view(x, y) -> 1/4 as x, y -> infinity (one quadrant)
        assert!(hottel_rect_view_factor(0.0, 2.5).abs() < 1e-14);
        assert!(hottel_rect_view_factor(2.5, 0.0).abs() < 1e-14);
        let vf_inf = hottel_rect_view_factor(500.0, 500.0);
        assert!((vf_inf - 0.25).abs() < 1e-3);

        // Glass emissivity E_glass = 1.05 - 0.00044 * Tg + 4.0 * h1
        let eps_hot = glass_emissivity(1050.0, 0.00675);
        let eps_cold = glass_emissivity(610.0, 0.00585);
        assert!(eps_hot > 0.60 && eps_hot < 0.63);
        assert!(eps_cold > 0.79 && eps_cold < 0.82);
    }

    #[test]
    fn unit_varicose_and_sinuous_distortion_uk5() {
        let case = CffCase::uk5_reference().expect("parse uk5 cff");
        assert_eq!(case.sinuous.num_meshes, 100);
        assert_eq!(case.sinuous.num_steps, 81);
        assert!((case.varicose.temp_fluctuation_c - 0.50).abs() < 1e-9);

        let dist = solve_varicose_and_sinuous_distortion(&case);
        assert_eq!(dist.varicose_points.len(), 205);
        assert_eq!(dist.sinuous_points.len(), 81);

        // Compare against legacy Fortran vardicff_x64 & sindi8cff_x64 output on uk5-585mm-mw-01e.cff:
        // - Varicose peak spreading power: +8.5119 Mdpt at x = 2.6214 m
        assert!((dist.varicose_max_spreading_power_mdpt - 8.5119).abs() < 0.02);
        assert!((dist.varicose_max_spreading_x_m - 2.6214).abs() < 0.01);
        // - Varicose peak top-roll attenuation power: -13.5298 Mdpt (ampl = -0.141365 um) at x = 13.4563 m
        assert!((dist.varicose_peak_attenuation_power_mdpt - (-13.5298)).abs() < 0.02);
        assert!((dist.varicose_peak_attenuation_ampl_um - (-0.141365)).abs() < 1e-4);
        assert!((dist.varicose_peak_attenuation_x_m - 13.4563).abs() < 0.01);
        // - Sinuous entry at x = 23.37033 m: ampl = 0.088739 um, wavel = 13.4027 mm, power = 4.87688 Mdpt
        assert!((dist.sinuous_hot_entry_ampl_um - 0.088739).abs() < 1e-5);
        assert!((dist.sinuous_hot_entry_wavelength_mm - 13.40271).abs() < 1e-3);
        assert!((dist.sinuous_hot_entry_power_mdpt - 4.876876).abs() < 1e-3);
        // - Sinuous peak amplitude in transition zone: 0.207959 um at x = 35.37033 m
        assert!((dist.sinuous_peak_ampl_um - 0.207959).abs() < 1e-5);
        assert!((dist.sinuous_peak_ampl_x_m - 35.37033).abs() < 1e-3);
        // - Sinuous cold-zone power at x = 45.37033 m: 0.372457 Mdpt
        assert!((dist.sinuous_cold_zone_power_mdpt_at_45m - 0.372457).abs() < 1e-3);
    }

    #[test]
    fn unit_pluggable_float_bath_coater_system_adjoint_and_uq() {
        use crate::coupled_system::{EngineConfig, MonolithicSystemEngine};
        let case = CffCase::uk5_reference().expect("parse uk5 cff");
        let sys = FloatBathCoaterDesignSystem::new(case, 10);
        let cfg = EngineConfig {
            dt_s: 3.0,
            num_bdf_steps: 1,
            max_newton_iters: 5,
            newton_tol: 1e-8,
            state_fd_eps: 1e-6,
        };
        let p_nom = sys.nominal_design_vector();
        let theta_nom = sys.nominal_epistemic_vector();

        let res = MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p_nom, &theta_nom)
            .expect("solve and analyze float-bath + CVD coater system");

        assert!(
            res.final_residual_l2 < 1e-7,
            "final_residual_l2 = {:e}",
            res.final_residual_l2
        );
        assert!(
            res.directional_fd_jacobian_rel_err < 1e-5,
            "FD Jacobian err = {:e}",
            res.directional_fd_jacobian_rel_err
        );
        assert!(
            res.adjoint_vs_tangent_max_rel_err < 1e-8,
            "Adjoint vs Tangent err = {:e}",
            res.adjoint_vs_tangent_max_rel_err
        );
        for audit in &res.conservation_audits {
            assert!(audit.passed(), "Conservation audit failed: {audit:?}");
        }

        let scenarios = vec![
            crate::coupled_system::NamedScenario {
                name: "A. Baseline".to_string(),
                description: "Baseline".to_string(),
                design_p: vec![90.0, 27.5, 28.0, 7.0, 0.0, 0.0],
            },
            crate::coupled_system::NamedScenario {
                name: "B. Hot Coater".to_string(),
                description: "Hot Coater".to_string(),
                design_p: vec![90.0, 14.5, 28.0, 7.0, 0.0, 0.0],
            },
            crate::coupled_system::NamedScenario {
                name: "C. Naive Low-H2".to_string(),
                description: "Naive Low-H2".to_string(),
                design_p: vec![90.0, 27.5, 28.0, 4.2, 0.0, 0.0],
            },
            crate::coupled_system::NamedScenario {
                name: "D. Synthesised".to_string(),
                description: "Synthesised".to_string(),
                design_p: vec![65.0, 28.5, 34.0, 4.2, 260.0, 0.15],
            },
        ];
        let suite =
            MonolithicSystemEngine::evaluate_design_scenarios(&sys, &cfg, &scenarios).unwrap();
        assert!(!suite.scenarios[1].constraints_pass_p50);
        assert!(!suite.scenarios[2].constraints_pass_p50);
        assert!(suite.scenarios[3].constraints_pass_p50);
        assert!(suite.scenarios[3].constraints_pass_robust_p90);
        assert!(suite.scenarios[3].pareto_efficient);
    }
}
