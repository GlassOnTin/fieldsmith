//! Composable declarative 0D/1D coupled-equation systems (`system <id> { ... }`) in `.fsm`.
//!
//! Compiles user-authored algebraic/ODE/1D-PDE conservation systems into typed bytecode
//! evaluated with forward-mode Dual-Number Automatic Differentiation (`Dual { val, dot }`),
//! and projects converged 0D scalars or 1D `lerp(<axis>, ...)` station profiles onto
//! document `let` parameters and 3D boundary conditions.

use crate::command::{apply_doc_command, DocCommand};
use crate::model::{Document, LoadKind};
use crate::spatial::{SpatialCoord, SpatialFn};
use serde::{Deserialize, Serialize};
use std::f64::consts::{E as EULER, PI};
use std::ops::{Add, Div, Mul, Neg, Sub};

/// World-space coordinate axis along which a 1D (`nodes >= 2`) system's station coordinate
/// `s ∈ [span[0], span[1]]` maps when projecting `lerp(...)` boundary profiles onto 3D surfaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DeclAxis {
    X,
    Y,
    #[default]
    Z,
    R,
}

impl DeclAxis {
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "x" => Some(Self::X),
            "y" => Some(Self::Y),
            "z" => Some(Self::Z),
            "r" => Some(Self::R),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Z => "z",
            Self::R => "r",
        }
    }

    pub fn to_spatial_coord(self) -> SpatialCoord {
        match self {
            Self::X => SpatialCoord::X,
            Self::Y => SpatialCoord::Y,
            Self::Z => SpatialCoord::Z,
            Self::R => SpatialCoord::R,
        }
    }
}

/// Engineering category of a controllable design lever `p_k`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DeclDesignCategory {
    Geometry,
    Hardware,
    Actuator,
    #[default]
    Process,
}

impl DeclDesignCategory {
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "geometry" => Some(Self::Geometry),
            "hardware" => Some(Self::Hardware),
            "actuator" => Some(Self::Actuator),
            "process" => Some(Self::Process),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Geometry => "geometry",
            Self::Hardware => "hardware",
            Self::Actuator => "actuator",
            Self::Process => "process",
        }
    }
}

/// Controllable design or operating parameter `p_k` declared inside `system`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeclDesignParam {
    pub name: String,
    pub nominal: f64,
    pub min: f64,
    pub max: f64,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub category: DeclDesignCategory,
    #[serde(default)]
    pub cost_rate: f64,
    #[serde(default)]
    pub description: String,
}

/// Epistemic constitutive or closure parameter `θ_l` (`nominal ± sigma`) declared inside `system`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeclEpistemicParam {
    pub name: String,
    pub nominal: f64,
    pub sigma: f64,
    pub min: f64,
    pub max: f64,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub mechanism: String,
}

/// Physical degree-of-freedom family `u_f` solved per station in `W`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeclStateField {
    pub name: String,
    pub init_expr: String,
    #[serde(default = "default_one")]
    pub scale: f64,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub symbol: String,
}

fn default_one() -> f64 {
    1.0
}

fn default_nodes() -> usize {
    1
}

fn default_span() -> [f64; 2] {
    [0.0, 1.0]
}

fn default_tol() -> f64 {
    0.02
}

/// Intermediate station expression `aux <name> expr="..."` evaluated in declaration order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeclAuxVar {
    pub name: String,
    pub expr: String,
}

/// Residual equation `R_f = 0` paired 1:1 with a [`DeclStateField`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeclResidual {
    pub state: String,
    pub eq: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bc_left: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bc_right: Option<String>,
    /// BDF1 pseudo-transient damping coefficient multiplying `(u - u_prev) * inv_dt`.
    #[serde(default)]
    pub transient: f64,
}

/// Station reduction operator mapping a field expression across `0..nodes` to a scalar objective.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DeclReduce {
    #[default]
    Value,
    Entry,
    Exit,
    Mean,
    Integral,
    Max,
    Min,
}

impl DeclReduce {
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "value" => Some(Self::Value),
            "entry" => Some(Self::Entry),
            "exit" => Some(Self::Exit),
            "mean" => Some(Self::Mean),
            "integral" => Some(Self::Integral),
            "max" => Some(Self::Max),
            "min" => Some(Self::Min),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Value => "value",
            Self::Entry => "entry",
            Self::Exit => "exit",
            Self::Mean => "mean",
            Self::Integral => "integral",
            Self::Max => "max",
            Self::Min => "min",
        }
    }
}

/// Optimisation / specification role of a [`DeclObjective`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DeclObjKind {
    Minimize,
    Maximize,
    Upper { limit: f64 },
    Lower { limit: f64 },
    Band { min: f64, max: f64 },
}

/// Engineering objective or constraint functional `Φ_m(W, p, θ)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeclObjective {
    pub name: String,
    pub expr: String,
    #[serde(default)]
    pub reduce: DeclReduce,
    pub kind: DeclObjKind,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub description: String,
}

/// Integral conservation balance audit evaluated on the converged state `W*`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeclAudit {
    pub name: String,
    pub inflow: String,
    pub outflow: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default = "default_tol")]
    pub tolerance: f64,
    #[serde(default)]
    pub unit: String,
}

/// Target attribute on a `.fsm` `let` or boundary condition statement written by [`DeclCadBinding`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclBindAttr {
    Let,
    Temp,
    H,
    Tinf,
    Q,
    Pressure,
    Preload,
    ForceX,
    ForceY,
    ForceZ,
}

impl DeclBindAttr {
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "let" => Some(Self::Let),
            "temp" => Some(Self::Temp),
            "h" => Some(Self::H),
            "tinf" => Some(Self::Tinf),
            "q" => Some(Self::Q),
            "pressure" => Some(Self::Pressure),
            "preload" => Some(Self::Preload),
            "force_x" => Some(Self::ForceX),
            "force_y" => Some(Self::ForceY),
            "force_z" => Some(Self::ForceZ),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Let => "let",
            Self::Temp => "temp",
            Self::H => "h",
            Self::Tinf => "tinf",
            Self::Q => "q",
            Self::Pressure => "pressure",
            Self::Preload => "preload",
            Self::ForceX => "force_x",
            Self::ForceY => "force_y",
            Self::ForceZ => "force_z",
        }
    }
}

/// Projection rule mapping a solved coupled expression onto a `.fsm` `let` or 3D boundary condition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeclCadBinding {
    pub target: String,
    pub attr: DeclBindAttr,
    pub expr: String,
}

/// Named design preset scenario inside `system`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeclScenario {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub design_p: Vec<f64>,
}

/// Declarative 0D/1D coupled multi-physics system block (`system <id> ... { ... }`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeclarativeSystem {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub sector: String,
    #[serde(default = "default_nodes")]
    pub nodes: usize,
    #[serde(default = "default_span")]
    pub span: [f64; 2],
    #[serde(default)]
    pub axis: DeclAxis,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub designs: Vec<DeclDesignParam>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub epistemics: Vec<DeclEpistemicParam>,
    pub states: Vec<DeclStateField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aux: Vec<DeclAuxVar>,
    pub residuals: Vec<DeclResidual>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objectives: Vec<DeclObjective>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audits: Vec<DeclAudit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<DeclCadBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenarios: Vec<DeclScenario>,
}

pub const MAX_DECL_SYSTEM_NODES: usize = 512;

impl DeclarativeSystem {
    /// Validate identifiers, bounds, 1:1 state-residual pairing, Dual-AD expression compilation,
    /// and 3D CAD binding targets against `doc`.
    pub fn validate(&self, doc: &Document) -> Vec<String> {
        let mut errs = Vec::new();
        let prefix = format!("System '{}'", self.id);
        if !crate::fsm::is_ident(&self.id) {
            errs.push(format!("{prefix}: id is not a valid identifier"));
        }
        if self.nodes == 0 || self.nodes > MAX_DECL_SYSTEM_NODES {
            errs.push(format!(
                "{prefix}: nodes={} must be in 1..={MAX_DECL_SYSTEM_NODES}",
                self.nodes
            ));
        }
        if !(self.span[0].is_finite() && self.span[1].is_finite()) {
            errs.push(format!("{prefix}: span must contain finite numbers"));
        } else if self.nodes >= 2 && self.span[1] <= self.span[0] {
            errs.push(format!(
                "{prefix}: 1D system (nodes={}) requires span[1] > span[0] (got {:?})",
                self.nodes, self.span
            ));
        }

        if self.states.is_empty() {
            errs.push(format!("{prefix}: must declare at least one 'state'"));
        }
        for dp in &self.designs {
            if !crate::fsm::is_ident(&dp.name) {
                errs.push(format!(
                    "{prefix}: design '{}' is not a valid identifier",
                    dp.name
                ));
            }
            if !(dp.nominal.is_finite()
                && dp.min.is_finite()
                && dp.max.is_finite()
                && dp.cost_rate.is_finite())
            {
                errs.push(format!(
                    "{prefix}: design '{}' bounds/nominal must be finite",
                    dp.name
                ));
            } else if dp.max <= dp.min {
                errs.push(format!(
                    "{prefix}: design '{}' requires max > min (got {} <= {})",
                    dp.name, dp.max, dp.min
                ));
            } else if dp.nominal < dp.min || dp.nominal > dp.max {
                errs.push(format!(
                    "{prefix}: design '{}' nominal ({}) lies outside [{} .. {}]",
                    dp.name, dp.nominal, dp.min, dp.max
                ));
            }
        }
        for ep in &self.epistemics {
            if !crate::fsm::is_ident(&ep.name) {
                errs.push(format!(
                    "{prefix}: epistemic '{}' is not a valid identifier",
                    ep.name
                ));
            }
            if !(ep.nominal.is_finite()
                && ep.sigma.is_finite()
                && ep.sigma >= 0.0
                && ep.min.is_finite()
                && ep.max.is_finite())
            {
                errs.push(format!(
                    "{prefix}: epistemic '{}' nominal/sigma/bounds must be finite (sigma >= 0)",
                    ep.name
                ));
            } else if ep.max <= ep.min {
                errs.push(format!(
                    "{prefix}: epistemic '{}' requires max > min (got {} <= {})",
                    ep.name, ep.max, ep.min
                ));
            }
        }
        for st in &self.states {
            if !crate::fsm::is_ident(&st.name) {
                errs.push(format!(
                    "{prefix}: state '{}' is not a valid identifier",
                    st.name
                ));
            }
            if !(st.scale.is_finite() && st.scale > 0.0) {
                errs.push(format!(
                    "{prefix}: state '{}' scale must be > 0 (got {})",
                    st.name, st.scale
                ));
            }
        }
        for sc in &self.scenarios {
            if sc.design_p.len() != self.designs.len() {
                errs.push(format!(
                    "{prefix}: scenario '{}' has {} design values, expected {}",
                    sc.name,
                    sc.design_p.len(),
                    self.designs.len()
                ));
            }
            if !sc.design_p.iter().all(|v| v.is_finite()) {
                errs.push(format!(
                    "{prefix}: scenario '{}' has a non-finite design value",
                    sc.name
                ));
            }
        }

        let let_vals: Vec<(String, f64)> =
            doc.lets.iter().map(|l| (l.name.clone(), l.value)).collect();
        if let Err(comp_errs) = CompiledSystem::compile(self, &let_vals) {
            errs.extend(comp_errs);
        }

        for b in &self.bindings {
            match b.attr {
                DeclBindAttr::Let => {
                    if !crate::fsm::is_ident(&b.target) {
                        errs.push(format!(
                            "{prefix}: bind target '{}' is not a valid let identifier",
                            b.target
                        ));
                    }
                }
                DeclBindAttr::Temp => {
                    if !doc.temperatures.iter().any(|t| t.id == b.target) {
                        errs.push(format!(
                            "{prefix}: bind attr=temp references missing temperature '{}'",
                            b.target
                        ));
                    }
                }
                DeclBindAttr::H | DeclBindAttr::Tinf => {
                    if !doc.convections.iter().any(|c| c.id == b.target) {
                        errs.push(format!(
                            "{prefix}: bind attr={} references missing convection '{}'",
                            b.attr.name(),
                            b.target
                        ));
                    }
                }
                DeclBindAttr::Q => {
                    if !doc.heat_fluxes.iter().any(|hf| hf.id == b.target) {
                        errs.push(format!(
                            "{prefix}: bind attr=q references missing heatflux '{}'",
                            b.target
                        ));
                    }
                }
                DeclBindAttr::Pressure => match doc.loads.iter().find(|l| l.id == b.target) {
                    Some(l) if matches!(l.kind, LoadKind::Pressure { .. }) => {}
                    Some(_) => errs.push(format!(
                        "{prefix}: bind attr=pressure requires pressure load '{}'",
                        b.target
                    )),
                    None => errs.push(format!(
                        "{prefix}: bind attr=pressure references missing load '{}'",
                        b.target
                    )),
                },
                DeclBindAttr::Preload => match doc.loads.iter().find(|l| l.id == b.target) {
                    Some(l) if matches!(l.kind, LoadKind::Preload { .. }) => {}
                    Some(_) => errs.push(format!(
                        "{prefix}: bind attr=preload requires preload load '{}'",
                        b.target
                    )),
                    None => errs.push(format!(
                        "{prefix}: bind attr=preload references missing load '{}'",
                        b.target
                    )),
                },
                DeclBindAttr::ForceX | DeclBindAttr::ForceY | DeclBindAttr::ForceZ => {
                    match doc.loads.iter().find(|l| l.id == b.target) {
                        Some(l) if matches!(l.kind, LoadKind::Force { .. }) => {}
                        Some(_) => errs.push(format!(
                            "{prefix}: bind attr={} requires force load '{}'",
                            b.attr.name(),
                            b.target
                        )),
                        None => errs.push(format!(
                            "{prefix}: bind attr={} references missing load '{}'",
                            b.attr.name(),
                            b.target
                        )),
                    }
                }
            }
        }

        errs
    }
}

// =============================================================================================
// Forward-Mode Dual Number (`val + ε * dot`)
// =============================================================================================

/// First-order forward-mode dual number `(val, dot)` for exact machine-precision AD.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dual {
    pub val: f64,
    pub dot: f64,
}

impl Neg for Dual {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Self {
            val: -self.val,
            dot: -self.dot,
        }
    }
}

impl Add for Dual {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self {
            val: self.val + rhs.val,
            dot: self.dot + rhs.dot,
        }
    }
}

impl Sub for Dual {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self {
            val: self.val - rhs.val,
            dot: self.dot - rhs.dot,
        }
    }
}

impl Mul for Dual {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Self) -> Self {
        Self {
            val: self.val * rhs.val,
            dot: self.dot * rhs.val + self.val * rhs.dot,
        }
    }
}

impl Div for Dual {
    type Output = Self;
    #[inline]
    fn div(self, rhs: Self) -> Self {
        let inv = 1.0 / rhs.val;
        Self {
            val: self.val * inv,
            dot: (self.dot * rhs.val - self.val * rhs.dot) * inv * inv,
        }
    }
}

impl Dual {
    #[inline]
    pub const fn constant(val: f64) -> Self {
        Self { val, dot: 0.0 }
    }

    #[inline]
    pub const fn seed(val: f64, dot: f64) -> Self {
        Self { val, dot }
    }

    #[inline]
    pub fn powd(self, exp: Self) -> Self {
        if exp.dot == 0.0 {
            let p = exp.val;
            if p == 0.0 {
                return Self::constant(1.0);
            }
            if p == 1.0 {
                return self;
            }
            if p == 2.0 {
                return Self {
                    val: self.val * self.val,
                    dot: 2.0 * self.val * self.dot,
                };
            }
            let v = self.val.powf(p);
            let dv = if self.dot == 0.0 {
                0.0
            } else {
                p * self.val.powf(p - 1.0) * self.dot
            };
            return Self { val: v, dot: dv };
        }
        let v = self.val.powf(exp.val);
        let t1 = if self.dot == 0.0 {
            0.0
        } else {
            exp.val * self.val.powf(exp.val - 1.0) * self.dot
        };
        let t2 = if v == 0.0 {
            0.0
        } else {
            v * self.val.abs().max(1e-300).ln() * exp.dot
        };
        Self {
            val: v,
            dot: t1 + t2,
        }
    }

    #[inline]
    pub fn sin(self) -> Self {
        Self {
            val: self.val.sin(),
            dot: self.val.cos() * self.dot,
        }
    }

    #[inline]
    pub fn cos(self) -> Self {
        Self {
            val: self.val.cos(),
            dot: -self.val.sin() * self.dot,
        }
    }

    #[inline]
    pub fn tan(self) -> Self {
        let c = self.val.cos();
        Self {
            val: self.val.tan(),
            dot: self.dot / (c * c),
        }
    }

    #[inline]
    pub fn tanh(self) -> Self {
        let t = self.val.tanh();
        Self {
            val: t,
            dot: (1.0 - t * t) * self.dot,
        }
    }

    #[inline]
    pub fn exp(self) -> Self {
        let ev = self.val.exp();
        Self {
            val: ev,
            dot: ev * self.dot,
        }
    }

    #[inline]
    pub fn ln(self) -> Self {
        Self {
            val: self.val.ln(),
            dot: self.dot / self.val,
        }
    }

    #[inline]
    pub fn sqrt(self) -> Self {
        let s = self.val.sqrt();
        let ds = if self.dot == 0.0 {
            0.0
        } else {
            0.5 * self.dot / s.max(1e-300)
        };
        Self { val: s, dot: ds }
    }

    #[inline]
    pub fn abs(self) -> Self {
        if self.val >= 0.0 {
            self
        } else {
            self.neg()
        }
    }

    #[inline]
    pub fn min(self, rhs: Self) -> Self {
        if self.val <= rhs.val {
            self
        } else {
            rhs
        }
    }

    #[inline]
    pub fn max(self, rhs: Self) -> Self {
        if self.val >= rhs.val {
            self
        } else {
            rhs
        }
    }

    #[inline]
    pub fn clamp(self, lo: Self, hi: Self) -> Self {
        self.max(lo).min(hi)
    }
}

// =============================================================================================
// Compiled Dual-AD Expression & System Engine
// =============================================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SysFn1 {
    Sin,
    Cos,
    Tan,
    Tanh,
    Exp,
    Ln,
    Sqrt,
    Abs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SysFn2 {
    Min,
    Max,
    Pow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SysBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SysExpr {
    Const(f64),
    CoordS,
    SpanLen,
    GridStep,
    DocLet(usize),
    Design(usize),
    Epistemic(usize),
    State(usize),
    StatePrev(usize),
    Aux(usize),
    Neg(Box<SysExpr>),
    Bin(SysBinOp, Box<SysExpr>, Box<SysExpr>),
    Fn1(SysFn1, Box<SysExpr>),
    Fn2(SysFn2, Box<SysExpr>, Box<SysExpr>),
    Clamp(Box<SysExpr>, Box<SysExpr>, Box<SysExpr>),
    Dds(usize),
    DdsUpwind(usize),
    DdsDownwind(usize),
    D2ds2(usize),
    StationEntry(Box<SysExpr>),
    StationExit(Box<SysExpr>),
    StationMean(Box<SysExpr>),
    StationIntegral(Box<SysExpr>),
}

impl SysExpr {
    /// Whether this expression references `StationMean` or `StationIntegral` (coupling all stations).
    pub fn has_global_station_reduction(&self, aux_exprs: &[SysExpr]) -> bool {
        match self {
            Self::Const(_)
            | Self::CoordS
            | Self::SpanLen
            | Self::GridStep
            | Self::DocLet(_)
            | Self::Design(_)
            | Self::Epistemic(_)
            | Self::State(_)
            | Self::StatePrev(_)
            | Self::Dds(_)
            | Self::DdsUpwind(_)
            | Self::DdsDownwind(_)
            | Self::D2ds2(_) => false,
            Self::Aux(a) => aux_exprs
                .get(*a)
                .is_some_and(|ex| ex.has_global_station_reduction(&aux_exprs[..*a])),
            Self::Neg(x) | Self::Fn1(_, x) | Self::StationEntry(x) | Self::StationExit(x) => {
                x.has_global_station_reduction(aux_exprs)
            }
            Self::Bin(_, a, b) | Self::Fn2(_, a, b) => {
                a.has_global_station_reduction(aux_exprs)
                    || b.has_global_station_reduction(aux_exprs)
            }
            Self::Clamp(a, b, c) => {
                a.has_global_station_reduction(aux_exprs)
                    || b.has_global_station_reduction(aux_exprs)
                    || c.has_global_station_reduction(aux_exprs)
            }
            Self::StationMean(_) | Self::StationIntegral(_) => true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CompiledResidual {
    pub state_idx: usize,
    pub eq: SysExpr,
    pub bc_left: Option<SysExpr>,
    pub bc_right: Option<SysExpr>,
    pub transient: f64,
}

#[derive(Clone, Debug)]
pub struct CompiledObjective {
    pub expr: SysExpr,
    pub reduce: DeclReduce,
}

#[derive(Clone, Debug)]
pub struct CompiledAudit {
    pub inflow: SysExpr,
    pub outflow: SysExpr,
    pub source: Option<SysExpr>,
}

/// Pre-compiled, zero-allocation-lookup representation of a [`DeclarativeSystem`].
#[derive(Clone, Debug)]
pub struct CompiledSystem {
    pub nodes: usize,
    pub span: [f64; 2],
    pub span_len: f64,
    pub ds: f64,
    pub stations: Vec<f64>,
    pub doc_let_vals: Vec<f64>,
    pub n_fields: usize,
    pub init_exprs: Vec<SysExpr>,
    pub aux_exprs: Vec<SysExpr>,
    pub residuals: Vec<CompiledResidual>,
    pub objectives: Vec<CompiledObjective>,
    pub audits: Vec<CompiledAudit>,
    pub bind_exprs: Vec<SysExpr>,
    pub has_global_residual_coupling: bool,
}

#[derive(Clone, Copy)]
struct CompileScope<'a> {
    nodes: usize,
    doc_lets: &'a [(String, f64)],
    designs: &'a [DeclDesignParam],
    epistemics: &'a [DeclEpistemicParam],
    states: &'a [DeclStateField],
    aux: &'a [DeclAuxVar],
    allow_states: bool,
}

impl CompiledSystem {
    /// Compile and type-check all expressions in `sys` against `doc_lets`.
    pub fn compile(
        sys: &DeclarativeSystem,
        doc_lets: &[(String, f64)],
    ) -> Result<Self, Vec<String>> {
        let mut errs = Vec::new();
        let prefix = format!("System '{}'", sys.id);
        let nodes = sys.nodes.max(1);
        let span_len = (sys.span[1] - sys.span[0]).abs();
        let ds = if nodes >= 2 {
            (sys.span[1] - sys.span[0]) / ((nodes - 1) as f64)
        } else {
            span_len.max(1.0)
        };
        let stations: Vec<f64> = if nodes >= 2 {
            (0..nodes).map(|i| sys.span[0] + (i as f64) * ds).collect()
        } else {
            vec![sys.span[0]]
        };

        // Check duplicate names across design / epistemic / state / aux
        let mut seen_owned: Vec<(String, &'static str)> = Vec::new();
        for d in &sys.designs {
            check_name_collision(&prefix, &d.name, "design", &mut seen_owned, &mut errs);
        }
        for e in &sys.epistemics {
            check_name_collision(&prefix, &e.name, "epistemic", &mut seen_owned, &mut errs);
        }
        for st in &sys.states {
            check_name_collision(&prefix, &st.name, "state", &mut seen_owned, &mut errs);
            let prev_name = format!("{}_prev", st.name);
            check_name_collision(
                &prefix,
                &prev_name,
                "implicit state_prev",
                &mut seen_owned,
                &mut errs,
            );
        }
        for ax in &sys.aux {
            if !crate::fsm::is_ident(&ax.name) {
                errs.push(format!(
                    "{prefix}: aux '{}' is not a valid identifier",
                    ax.name
                ));
            }
            check_name_collision(&prefix, &ax.name, "aux", &mut seen_owned, &mut errs);
        }

        // Compile state `init_expr` (may reference `s`, `L`, `ds`, `lets`, `designs`, `epistemics`, NOT states/aux)
        let init_scope = CompileScope {
            nodes,
            doc_lets,
            designs: &sys.designs,
            epistemics: &sys.epistemics,
            states: &sys.states,
            aux: &[],
            allow_states: false,
        };
        let mut init_exprs = Vec::with_capacity(sys.states.len());
        for st in &sys.states {
            match compile_expr_str(&st.init_expr, &init_scope) {
                Ok(ex) => init_exprs.push(ex),
                Err(e) => errs.push(format!("{prefix} state '{}' init: {e}", st.name)),
            }
        }

        // Compile `aux` in declaration order (each `aux[k]` may reference `aux[0..k]` + states)
        let mut aux_exprs = Vec::with_capacity(sys.aux.len());
        for (k, ax) in sys.aux.iter().enumerate() {
            let scope = CompileScope {
                nodes,
                doc_lets,
                designs: &sys.designs,
                epistemics: &sys.epistemics,
                states: &sys.states,
                aux: &sys.aux[..k],
                allow_states: true,
            };
            match compile_expr_str(&ax.expr, &scope) {
                Ok(ex) => aux_exprs.push(ex),
                Err(e) => errs.push(format!("{prefix} aux '{}': {e}", ax.name)),
            }
        }

        let full_scope = CompileScope {
            nodes,
            doc_lets,
            designs: &sys.designs,
            epistemics: &sys.epistemics,
            states: &sys.states,
            aux: &sys.aux,
            allow_states: true,
        };

        // Verify 1:1 pairing between states and residuals (ordered by `sys.states` index!)
        if sys.residuals.len() != sys.states.len() {
            errs.push(format!(
                "{prefix}: declared {} residual equations for {} state fields (must match 1:1)",
                sys.residuals.len(),
                sys.states.len()
            ));
        }
        let mut residuals_by_state: Vec<Option<CompiledResidual>> = vec![None; sys.states.len()];
        for res in &sys.residuals {
            let Some(s_idx) = sys.states.iter().position(|s| s.name == res.state) else {
                errs.push(format!(
                    "{prefix}: residual targets unknown state '{}'",
                    res.state
                ));
                continue;
            };
            if residuals_by_state[s_idx].is_some() {
                errs.push(format!(
                    "{prefix}: duplicate residual equation for state '{}'",
                    res.state
                ));
                continue;
            }
            if !res.transient.is_finite() {
                errs.push(format!(
                    "{prefix}: residual '{}' transient coefficient must be finite",
                    res.state
                ));
            }
            if nodes == 1 && (res.bc_left.is_some() || res.bc_right.is_some()) {
                errs.push(format!(
                    "{prefix}: residual '{}' specifies bc_left/bc_right on a 0D (nodes=1) system",
                    res.state
                ));
            }
            let eq = match compile_expr_str(&res.eq, &full_scope) {
                Ok(ex) => Some(ex),
                Err(e) => {
                    errs.push(format!("{prefix} residual '{}' eq: {e}", res.state));
                    None
                }
            };
            let bc_left = match &res.bc_left {
                Some(src) => match compile_expr_str(src, &full_scope) {
                    Ok(ex) => Some(ex),
                    Err(e) => {
                        errs.push(format!("{prefix} residual '{}' bc_left: {e}", res.state));
                        None
                    }
                },
                None => None,
            };
            let bc_right = match &res.bc_right {
                Some(src) => match compile_expr_str(src, &full_scope) {
                    Ok(ex) => Some(ex),
                    Err(e) => {
                        errs.push(format!("{prefix} residual '{}' bc_right: {e}", res.state));
                        None
                    }
                },
                None => None,
            };
            if let Some(eq) = eq {
                residuals_by_state[s_idx] = Some(CompiledResidual {
                    state_idx: s_idx,
                    eq,
                    bc_left,
                    bc_right,
                    transient: res.transient,
                });
            }
        }

        let mut objectives = Vec::with_capacity(sys.objectives.len());
        for obj in &sys.objectives {
            if !crate::fsm::is_ident(&obj.name) {
                errs.push(format!(
                    "{prefix}: objective '{}' is not a valid identifier",
                    obj.name
                ));
            }
            match obj.kind {
                DeclObjKind::Upper { limit } | DeclObjKind::Lower { limit } => {
                    if !limit.is_finite() {
                        errs.push(format!(
                            "{prefix}: objective '{}' limit must be finite",
                            obj.name
                        ));
                    }
                }
                DeclObjKind::Band { min, max } => {
                    if !(min.is_finite() && max.is_finite() && max > min) {
                        errs.push(format!(
                            "{prefix}: objective '{}' band requires finite max > min",
                            obj.name
                        ));
                    }
                }
                DeclObjKind::Minimize | DeclObjKind::Maximize => {}
            }
            match compile_expr_str(&obj.expr, &full_scope) {
                Ok(expr) => objectives.push(CompiledObjective {
                    expr,
                    reduce: obj.reduce,
                }),
                Err(e) => errs.push(format!("{prefix} objective '{}': {e}", obj.name)),
            }
        }

        let mut audits = Vec::with_capacity(sys.audits.len());
        for aud in &sys.audits {
            if !(aud.tolerance.is_finite() && aud.tolerance > 0.0) {
                errs.push(format!(
                    "{prefix}: audit '{}' tolerance must be > 0",
                    aud.name
                ));
            }
            let inf = compile_expr_str(&aud.inflow, &full_scope)
                .map_err(|e| errs.push(format!("{prefix} audit '{}' inflow: {e}", aud.name)))
                .ok();
            let outf = compile_expr_str(&aud.outflow, &full_scope)
                .map_err(|e| errs.push(format!("{prefix} audit '{}' outflow: {e}", aud.name)))
                .ok();
            let src = match &aud.source {
                Some(s) => match compile_expr_str(s, &full_scope) {
                    Ok(ex) => Some(ex),
                    Err(e) => {
                        errs.push(format!("{prefix} audit '{}' source: {e}", aud.name));
                        None
                    }
                },
                None => None,
            };
            if let (Some(inflow), Some(outflow)) = (inf, outf) {
                audits.push(CompiledAudit {
                    inflow,
                    outflow,
                    source: src,
                });
            }
        }

        let mut bind_exprs = Vec::with_capacity(sys.bindings.len());
        for b in &sys.bindings {
            match compile_expr_str(&b.expr, &full_scope) {
                Ok(ex) => bind_exprs.push(ex),
                Err(e) => errs.push(format!(
                    "{prefix} bind '{}' ({}): {e}",
                    b.target,
                    b.attr.name()
                )),
            }
        }

        if !errs.is_empty() {
            return Err(errs);
        }

        let residuals: Vec<CompiledResidual> = residuals_by_state
            .into_iter()
            .map(|r| r.expect("checked 1:1 state-residual pairing"))
            .collect();
        let has_global_residual_coupling = residuals.iter().any(|r| {
            r.eq.has_global_station_reduction(&aux_exprs)
                || r.bc_left
                    .as_ref()
                    .is_some_and(|e| e.has_global_station_reduction(&aux_exprs))
                || r.bc_right
                    .as_ref()
                    .is_some_and(|e| e.has_global_station_reduction(&aux_exprs))
        });

        Ok(Self {
            nodes,
            span: sys.span,
            span_len,
            ds,
            stations,
            doc_let_vals: doc_lets.iter().map(|(_, v)| *v).collect(),
            n_fields: sys.states.len(),
            init_exprs,
            aux_exprs,
            residuals,
            objectives,
            audits,
            bind_exprs,
            has_global_residual_coupling,
        })
    }

    #[inline]
    pub fn num_dofs(&self) -> usize {
        self.nodes * self.n_fields
    }

    /// Evaluate the initial state vector `W_0(p, θ)` of length `nodes * n_fields`.
    pub fn eval_initial_state(&self, design_p: &[f64], epistemic_theta: &[f64]) -> Vec<f64> {
        let p_dual: Vec<Dual> = design_p.iter().map(|&v| Dual::constant(v)).collect();
        let th_dual: Vec<Dual> = epistemic_theta.iter().map(|&v| Dual::constant(v)).collect();
        let empty: [Dual; 0] = [];
        let ctx = EvalCtx {
            sys: self,
            w: &empty,
            w_prev: &empty,
            design_p: &p_dual,
            epistemic_theta: &th_dual,
            aux_grid: &[],
        };
        let mut out = vec![0.0; self.num_dofs()];
        for i in 0..self.nodes {
            for f in 0..self.n_fields {
                out[i * self.n_fields + f] = ctx.eval(&self.init_exprs[f], i).val;
            }
        }
        out
    }

    /// Precompute all `aux` variables on the station grid `0..nodes` for Dual state/param vectors.
    fn compute_aux_grid(
        &self,
        w: &[Dual],
        w_prev: &[Dual],
        design_p: &[Dual],
        epistemic_theta: &[Dual],
    ) -> Vec<Dual> {
        let n_aux = self.aux_exprs.len();
        if n_aux == 0 {
            return Vec::new();
        }
        let mut aux_grid = vec![Dual::default(); self.nodes * n_aux];
        // Evaluate `aux[a]` across ALL stations `i = 0..nodes` before `aux[a + 1]`, so
        // `aux[a + 1]` can safely apply `entry(...)`, `exit(...)`, `mean(...)`, or `integral(...)`
        // to `aux[a]`!
        for a in 0..n_aux {
            for i in 0..self.nodes {
                let val = {
                    let ctx = EvalCtx {
                        sys: self,
                        w,
                        w_prev,
                        design_p,
                        epistemic_theta,
                        aux_grid: &aux_grid,
                    };
                    ctx.eval(&self.aux_exprs[a], i)
                };
                aux_grid[i * n_aux + a] = val;
            }
        }
        aux_grid
    }

    /// Evaluate residual `R` at a single station `i` in Dual arithmetic.
    #[inline]
    fn eval_station_residuals(
        &self,
        ctx: &EvalCtx<'_>,
        i: usize,
        inv_dt: f64,
        out_station: &mut [Dual],
    ) {
        for (f, res) in self.residuals.iter().enumerate() {
            let expr = if self.nodes >= 2 && i == 0 {
                res.bc_left.as_ref().unwrap_or(&res.eq)
            } else if self.nodes >= 2 && i + 1 == self.nodes {
                res.bc_right.as_ref().unwrap_or(&res.eq)
            } else {
                &res.eq
            };
            let mut r = ctx.eval(expr, i);
            let is_dirichlet_or_neumann_bc = (self.nodes >= 2 && i == 0 && res.bc_left.is_some())
                || (self.nodes >= 2 && i + 1 == self.nodes && res.bc_right.is_some());
            if inv_dt != 0.0 && res.transient != 0.0 && !is_dirichlet_or_neumann_bc {
                let idx = i * self.n_fields + res.state_idx;
                let du = ctx.w[idx].sub(ctx.w_prev[idx]);
                r = r.add(Dual::constant(res.transient * inv_dt).mul(du));
            }
            out_station[f] = r;
        }
    }

    /// Evaluate the monolithic residual vector `R(W, W_prev, inv_dt; p, θ)`.
    pub fn eval_residual(
        &self,
        w: &[f64],
        w_prev: &[f64],
        inv_dt: f64,
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<f64> {
        let w_d: Vec<Dual> = w.iter().map(|&v| Dual::constant(v)).collect();
        let wp_d: Vec<Dual> = w_prev.iter().map(|&v| Dual::constant(v)).collect();
        let p_d: Vec<Dual> = design_p.iter().map(|&v| Dual::constant(v)).collect();
        let th_d: Vec<Dual> = epistemic_theta.iter().map(|&v| Dual::constant(v)).collect();
        let aux_grid = self.compute_aux_grid(&w_d, &wp_d, &p_d, &th_d);
        let ctx = EvalCtx {
            sys: self,
            w: &w_d,
            w_prev: &wp_d,
            design_p: &p_d,
            epistemic_theta: &th_d,
            aux_grid: &aux_grid,
        };
        let mut r = vec![0.0; self.num_dofs()];
        let mut station_buf = vec![Dual::default(); self.n_fields];
        for i in 0..self.nodes {
            self.eval_station_residuals(&ctx, i, inv_dt, &mut station_buf);
            for f in 0..self.n_fields {
                r[i * self.n_fields + f] = station_buf[f].val;
            }
        }
        r
    }

    /// Assemble the exact residual `R` and sparse Jacobian triplets `(row, col, J_rc)` using
    /// forward-mode Dual-Number Automatic Differentiation (`dot = 1.0` seeded per column),
    /// returning `(R, triplets, cross_physics_nnz)`.
    pub fn assemble_jacobian_triplets(
        &self,
        w: &[f64],
        w_prev: &[f64],
        inv_dt: f64,
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> (Vec<f64>, Vec<(usize, usize, f64)>, usize) {
        let ndof = self.num_dofs();
        let n_fields = self.n_fields.max(1);
        let r0 = self.eval_residual(w, w_prev, inv_dt, design_p, epistemic_theta);

        let mut w_d: Vec<Dual> = w.iter().map(|&v| Dual::constant(v)).collect();
        let wp_d: Vec<Dual> = w_prev.iter().map(|&v| Dual::constant(v)).collect();
        let p_d: Vec<Dual> = design_p.iter().map(|&v| Dual::constant(v)).collect();
        let th_d: Vec<Dual> = epistemic_theta.iter().map(|&v| Dual::constant(v)).collect();

        let mut triplets: Vec<(usize, usize, f64)> = Vec::with_capacity(ndof * (3 * n_fields + 2));
        let mut cross_physics_nnz = 0usize;
        let mut station_buf = vec![Dual::default(); self.n_fields];
        let mut candidate_stations: Vec<usize> = Vec::with_capacity(self.nodes);

        for col in 0..ndof {
            let col_station = col / n_fields;
            let col_field = col % n_fields;
            w_d[col].dot = 1.0;

            candidate_stations.clear();
            if self.has_global_residual_coupling || self.nodes <= 5 {
                candidate_stations.extend(0..self.nodes);
            } else {
                // Local 3-point stencil `[j-1, j, j+1]` plus boundary stations `0` and `N-1`
                // (in case boundary conditions or interior terms reference `entry(...)`/`exit(...)`
                // or one-sided 2nd-order boundary stencils `[0, 1, 2]` / `[N-3, N-2, N-1]`).
                let mut push_unique = |s: usize| {
                    if s < self.nodes && !candidate_stations.contains(&s) {
                        candidate_stations.push(s);
                    }
                };
                push_unique(0);
                if col_station >= 2 {
                    push_unique(col_station - 2);
                }
                if col_station >= 1 {
                    push_unique(col_station - 1);
                }
                push_unique(col_station);
                push_unique(col_station + 1);
                push_unique(col_station + 2);
                push_unique(self.nodes - 1);
                candidate_stations.sort_unstable();
            }

            let aux_grid = if self.aux_exprs.is_empty() {
                Vec::new()
            } else if self.has_global_residual_coupling || self.nodes <= 5 {
                self.compute_aux_grid(&w_d, &wp_d, &p_d, &th_d)
            } else {
                let n_aux = self.aux_exprs.len();
                let mut ag = vec![Dual::default(); self.nodes * n_aux];
                for a in 0..n_aux {
                    for &s_idx in &candidate_stations {
                        let val = {
                            let ctx = EvalCtx {
                                sys: self,
                                w: &w_d,
                                w_prev: &wp_d,
                                design_p: &p_d,
                                epistemic_theta: &th_d,
                                aux_grid: &ag,
                            };
                            ctx.eval(&self.aux_exprs[a], s_idx)
                        };
                        ag[s_idx * n_aux + a] = val;
                    }
                }
                ag
            };

            let ctx = EvalCtx {
                sys: self,
                w: &w_d,
                w_prev: &wp_d,
                design_p: &p_d,
                epistemic_theta: &th_d,
                aux_grid: &aux_grid,
            };

            for &row_station in &candidate_stations {
                self.eval_station_residuals(&ctx, row_station, inv_dt, &mut station_buf);
                for (f, r_dual) in station_buf.iter().enumerate() {
                    let row = row_station * n_fields + f;
                    let dr = r_dual.dot;
                    if dr.abs() > 1e-15 || row == col {
                        let val = if row == col && dr.abs() <= 1e-15 {
                            1e-12
                        } else {
                            dr
                        };
                        triplets.push((row, col, val));
                        if f != col_field {
                            cross_physics_nnz += 1;
                        }
                    }
                }
            }

            w_d[col].dot = 0.0;
        }

        (r0, triplets, cross_physics_nnz)
    }

    /// Evaluate all declared objectives `Φ_m(W, p, θ)`.
    pub fn eval_objectives(
        &self,
        w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<f64> {
        let w_d: Vec<Dual> = w.iter().map(|&v| Dual::constant(v)).collect();
        let p_d: Vec<Dual> = design_p.iter().map(|&v| Dual::constant(v)).collect();
        let th_d: Vec<Dual> = epistemic_theta.iter().map(|&v| Dual::constant(v)).collect();
        let aux_grid = self.compute_aux_grid(&w_d, &w_d, &p_d, &th_d);
        let ctx = EvalCtx {
            sys: self,
            w: &w_d,
            w_prev: &w_d,
            design_p: &p_d,
            epistemic_theta: &th_d,
            aux_grid: &aux_grid,
        };

        self.objectives
            .iter()
            .map(|obj| self.reduce_expr(&ctx, &obj.expr, obj.reduce).val)
            .collect()
    }

    /// Evaluate conservation balance tuples `(inflow, outflow, source_or_sink)` for each `DeclAudit`.
    pub fn eval_audits(
        &self,
        w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<(f64, f64, f64)> {
        let w_d: Vec<Dual> = w.iter().map(|&v| Dual::constant(v)).collect();
        let p_d: Vec<Dual> = design_p.iter().map(|&v| Dual::constant(v)).collect();
        let th_d: Vec<Dual> = epistemic_theta.iter().map(|&v| Dual::constant(v)).collect();
        let aux_grid = self.compute_aux_grid(&w_d, &w_d, &p_d, &th_d);
        let ctx = EvalCtx {
            sys: self,
            w: &w_d,
            w_prev: &w_d,
            design_p: &p_d,
            epistemic_theta: &th_d,
            aux_grid: &aux_grid,
        };

        self.audits
            .iter()
            .map(|aud| {
                let inflow = ctx.eval(&aud.inflow, 0).val;
                let outflow = ctx.eval(&aud.outflow, self.nodes - 1).val;
                let source = aud
                    .source
                    .as_ref()
                    .map(|s| ctx.eval(s, 0).val)
                    .unwrap_or(0.0);
                (inflow, outflow, source)
            })
            .collect()
    }

    /// Evaluate each `bind` expression across stations `0..nodes`, returning `Vec<Vec<f64>>`.
    pub fn eval_bindings(
        &self,
        w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<Vec<f64>> {
        let w_d: Vec<Dual> = w.iter().map(|&v| Dual::constant(v)).collect();
        let p_d: Vec<Dual> = design_p.iter().map(|&v| Dual::constant(v)).collect();
        let th_d: Vec<Dual> = epistemic_theta.iter().map(|&v| Dual::constant(v)).collect();
        let aux_grid = self.compute_aux_grid(&w_d, &w_d, &p_d, &th_d);
        let ctx = EvalCtx {
            sys: self,
            w: &w_d,
            w_prev: &w_d,
            design_p: &p_d,
            epistemic_theta: &th_d,
            aux_grid: &aux_grid,
        };
        self.bind_exprs
            .iter()
            .map(|b_ex| (0..self.nodes).map(|i| ctx.eval(b_ex, i).val).collect())
            .collect()
    }

    fn reduce_expr(&self, ctx: &EvalCtx<'_>, expr: &SysExpr, reduce: DeclReduce) -> Dual {
        match reduce {
            DeclReduce::Value | DeclReduce::Entry => ctx.eval(expr, 0),
            DeclReduce::Exit => ctx.eval(expr, self.nodes - 1),
            DeclReduce::Mean => {
                let mut acc = Dual::constant(0.0);
                for i in 0..self.nodes {
                    acc = acc.add(ctx.eval(expr, i));
                }
                acc.div(Dual::constant(self.nodes as f64))
            }
            DeclReduce::Integral => {
                if self.nodes == 1 {
                    return ctx.eval(expr, 0).mul(Dual::constant(self.span_len));
                }
                let mut acc = Dual::constant(0.0);
                for i in 0..self.nodes {
                    let wt = if i == 0 || i + 1 == self.nodes {
                        0.5 * self.ds
                    } else {
                        self.ds
                    };
                    acc = acc.add(ctx.eval(expr, i).mul(Dual::constant(wt)));
                }
                acc
            }
            DeclReduce::Max => {
                let mut best = ctx.eval(expr, 0);
                for i in 1..self.nodes {
                    let v = ctx.eval(expr, i);
                    if v.val > best.val {
                        best = v;
                    }
                }
                best
            }
            DeclReduce::Min => {
                let mut best = ctx.eval(expr, 0);
                for i in 1..self.nodes {
                    let v = ctx.eval(expr, i);
                    if v.val < best.val {
                        best = v;
                    }
                }
                best
            }
        }
    }
}

struct EvalCtx<'a> {
    sys: &'a CompiledSystem,
    w: &'a [Dual],
    w_prev: &'a [Dual],
    design_p: &'a [Dual],
    epistemic_theta: &'a [Dual],
    aux_grid: &'a [Dual],
}

impl<'a> EvalCtx<'a> {
    #[inline]
    fn state_at(&self, station: usize, field: usize) -> Dual {
        self.w[station * self.sys.n_fields + field]
    }

    fn eval(&self, expr: &SysExpr, i: usize) -> Dual {
        match expr {
            SysExpr::Const(v) => Dual::constant(*v),
            SysExpr::CoordS => Dual::constant(self.sys.stations[i]),
            SysExpr::SpanLen => Dual::constant(self.sys.span_len),
            SysExpr::GridStep => Dual::constant(self.sys.ds),
            SysExpr::DocLet(idx) => Dual::constant(self.sys.doc_let_vals[*idx]),
            SysExpr::Design(idx) => self.design_p[*idx],
            SysExpr::Epistemic(idx) => self.epistemic_theta[*idx],
            SysExpr::State(f) => self.state_at(i, *f),
            SysExpr::StatePrev(f) => self.w_prev[i * self.sys.n_fields + *f],
            SysExpr::Aux(a) => self.aux_grid[i * self.sys.aux_exprs.len() + *a],
            SysExpr::Neg(x) => self.eval(x, i).neg(),
            SysExpr::Bin(op, a, b) => {
                let lv = self.eval(a, i);
                let rv = self.eval(b, i);
                match op {
                    SysBinOp::Add => lv.add(rv),
                    SysBinOp::Sub => lv.sub(rv),
                    SysBinOp::Mul => lv.mul(rv),
                    SysBinOp::Div => lv.div(rv),
                    SysBinOp::Pow => lv.powd(rv),
                }
            }
            SysExpr::Fn1(f, x) => {
                let xv = self.eval(x, i);
                match f {
                    SysFn1::Sin => xv.sin(),
                    SysFn1::Cos => xv.cos(),
                    SysFn1::Tan => xv.tan(),
                    SysFn1::Tanh => xv.tanh(),
                    SysFn1::Exp => xv.exp(),
                    SysFn1::Ln => xv.ln(),
                    SysFn1::Sqrt => xv.sqrt(),
                    SysFn1::Abs => xv.abs(),
                }
            }
            SysExpr::Fn2(f, a, b) => {
                let av = self.eval(a, i);
                let bv = self.eval(b, i);
                match f {
                    SysFn2::Min => av.min(bv),
                    SysFn2::Max => av.max(bv),
                    SysFn2::Pow => av.powd(bv),
                }
            }
            SysExpr::Clamp(x, lo, hi) => self.eval(x, i).clamp(self.eval(lo, i), self.eval(hi, i)),
            SysExpr::Dds(f) => {
                let n = self.sys.nodes;
                if n < 2 {
                    Dual::constant(0.0)
                } else if n == 2 {
                    self.state_at(1, *f)
                        .sub(self.state_at(0, *f))
                        .div(Dual::constant(self.sys.ds))
                } else if i == 0 {
                    (Dual::constant(-3.0) * self.state_at(0, *f)
                        + Dual::constant(4.0) * self.state_at(1, *f)
                        - self.state_at(2, *f))
                        / Dual::constant(2.0 * self.sys.ds)
                } else if i + 1 == n {
                    (Dual::constant(3.0) * self.state_at(n - 1, *f)
                        - Dual::constant(4.0) * self.state_at(n - 2, *f)
                        + self.state_at(n - 3, *f))
                        / Dual::constant(2.0 * self.sys.ds)
                } else {
                    (self.state_at(i + 1, *f) - self.state_at(i - 1, *f))
                        / Dual::constant(2.0 * self.sys.ds)
                }
            }
            SysExpr::DdsUpwind(f) => {
                let n = self.sys.nodes;
                if n < 2 {
                    Dual::constant(0.0)
                } else if i == 0 {
                    self.state_at(1, *f)
                        .sub(self.state_at(0, *f))
                        .div(Dual::constant(self.sys.ds))
                } else {
                    self.state_at(i, *f)
                        .sub(self.state_at(i - 1, *f))
                        .div(Dual::constant(self.sys.ds))
                }
            }
            SysExpr::DdsDownwind(f) => {
                let n = self.sys.nodes;
                if n < 2 {
                    Dual::constant(0.0)
                } else if i + 1 == n {
                    self.state_at(n - 1, *f)
                        .sub(self.state_at(n - 2, *f))
                        .div(Dual::constant(self.sys.ds))
                } else {
                    self.state_at(i + 1, *f)
                        .sub(self.state_at(i, *f))
                        .div(Dual::constant(self.sys.ds))
                }
            }
            SysExpr::D2ds2(f) => {
                let n = self.sys.nodes;
                if n < 3 {
                    Dual::constant(0.0)
                } else {
                    let c = if i == 0 {
                        1
                    } else if i + 1 == n {
                        n - 2
                    } else {
                        i
                    };
                    let u_p = self.state_at(c + 1, *f);
                    let u_0 = self.state_at(c, *f);
                    let u_m = self.state_at(c - 1, *f);
                    let ds2 = self.sys.ds * self.sys.ds;
                    u_p.sub(u_0.mul(Dual::constant(2.0)))
                        .add(u_m)
                        .div(Dual::constant(ds2))
                }
            }
            SysExpr::StationEntry(sub) => self.eval(sub, 0),
            SysExpr::StationExit(sub) => self.eval(sub, self.sys.nodes - 1),
            SysExpr::StationMean(sub) => self.sys.reduce_expr(self, sub, DeclReduce::Mean),
            SysExpr::StationIntegral(sub) => self.sys.reduce_expr(self, sub, DeclReduce::Integral),
        }
    }
}

fn check_name_collision(
    prefix: &str,
    name: &str,
    kind: &'static str,
    seen: &mut Vec<(String, &'static str)>,
    errs: &mut Vec<String>,
) {
    if matches!(name, "s" | "L" | "ds" | "pi" | "e") {
        errs.push(format!(
            "{prefix}: {kind} '{name}' collides with reserved symbol '{name}'"
        ));
    } else if let Some((_, prev_kind)) = seen.iter().find(|(n, _)| n == name) {
        errs.push(format!(
            "{prefix}: {kind} '{name}' duplicates earlier {prev_kind} '{name}'"
        ));
    } else {
        seen.push((name.to_string(), kind));
    }
}

// =============================================================================================
// Expression Tokenizer & Recursive-Descent Parser
// =============================================================================================

#[derive(Clone, Debug, PartialEq)]
enum ExprTok {
    Num(f64),
    Ident(String),
    Sym(char),
}

fn tokenize_sys_expr(src: &str) -> Result<Vec<ExprTok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut e_i = i + 1;
                if e_i < chars.len() && (chars[e_i] == '+' || chars[e_i] == '-') {
                    e_i += 1;
                }
                if e_i < chars.len() && chars[e_i].is_ascii_digit() {
                    i = e_i + 1;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text: String = chars[start..i].iter().collect();
            let v: f64 = text
                .parse()
                .map_err(|_| format!("invalid number literal '{text}'"))?;
            out.push(ExprTok::Num(v));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(ExprTok::Ident(chars[start..i].iter().collect()));
        } else if "+-*/^(),".contains(c) {
            out.push(ExprTok::Sym(c));
            i += 1;
        } else {
            return Err(format!("unexpected character '{c}' in expression"));
        }
    }
    Ok(out)
}

struct ExprParser<'a> {
    toks: &'a [ExprTok],
    pos: usize,
    scope: &'a CompileScope<'a>,
}

impl<'a> ExprParser<'a> {
    fn peek(&self) -> Option<&ExprTok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<ExprTok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn is_sym(&self, c: char) -> bool {
        matches!(self.peek(), Some(ExprTok::Sym(s)) if *s == c)
    }

    fn expect_sym(&mut self, c: char) -> Result<(), String> {
        match self.next() {
            Some(ExprTok::Sym(s)) if s == c => Ok(()),
            other => Err(format!("expected '{c}', found {other:?}")),
        }
    }

    fn parse_sum(&mut self) -> Result<SysExpr, String> {
        let mut left = self.parse_product()?;
        while matches!(self.peek(), Some(ExprTok::Sym('+' | '-'))) {
            let op = match self.next() {
                Some(ExprTok::Sym('+')) => SysBinOp::Add,
                Some(ExprTok::Sym('-')) => SysBinOp::Sub,
                _ => unreachable!(),
            };
            let right = self.parse_product()?;
            left = SysExpr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_product(&mut self) -> Result<SysExpr, String> {
        let mut left = self.parse_unary()?;
        while matches!(self.peek(), Some(ExprTok::Sym('*' | '/'))) {
            let op = match self.next() {
                Some(ExprTok::Sym('*')) => SysBinOp::Mul,
                Some(ExprTok::Sym('/')) => SysBinOp::Div,
                _ => unreachable!(),
            };
            let right = self.parse_unary()?;
            left = SysExpr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<SysExpr, String> {
        if self.is_sym('-') {
            self.next();
            return Ok(SysExpr::Neg(Box::new(self.parse_unary()?)));
        }
        if self.is_sym('+') {
            self.next();
            return self.parse_unary();
        }
        self.parse_power()
    }

    fn parse_power(&mut self) -> Result<SysExpr, String> {
        let base = self.parse_atom()?;
        if self.is_sym('^') {
            self.next();
            let exp = self.parse_unary()?;
            Ok(SysExpr::Bin(SysBinOp::Pow, Box::new(base), Box::new(exp)))
        } else {
            Ok(base)
        }
    }

    fn expect_state_arg(&mut self, fn_name: &str) -> Result<usize, String> {
        if !self.scope.allow_states {
            return Err(format!(
                "spatial operator '{fn_name}' cannot be used in initial condition expressions"
            ));
        }
        if self.scope.nodes < 2 {
            return Err(format!(
                "spatial operator '{fn_name}' requires a 1D system (nodes >= 2)"
            ));
        }
        self.expect_sym('(')?;
        let state_name = match self.next() {
            Some(ExprTok::Ident(s)) => s,
            other => {
                return Err(format!(
                    "{fn_name}(...) expects a state field name, found {other:?}"
                ))
            }
        };
        self.expect_sym(')')?;
        self.scope
            .states
            .iter()
            .position(|st| st.name == state_name)
            .ok_or_else(|| format!("{fn_name}({state_name}): unknown state field '{state_name}'"))
    }

    fn parse_atom(&mut self) -> Result<SysExpr, String> {
        match self.next() {
            Some(ExprTok::Num(v)) => Ok(SysExpr::Const(v)),
            Some(ExprTok::Sym('(')) => {
                let inner = self.parse_sum()?;
                self.expect_sym(')')?;
                Ok(inner)
            }
            Some(ExprTok::Ident(name)) => {
                if self.is_sym('(') {
                    match name.as_str() {
                        "d_ds" => return Ok(SysExpr::Dds(self.expect_state_arg("d_ds")?)),
                        "d_ds_upwind" => {
                            return Ok(SysExpr::DdsUpwind(self.expect_state_arg("d_ds_upwind")?))
                        }
                        "d_ds_downwind" => {
                            return Ok(SysExpr::DdsDownwind(
                                self.expect_state_arg("d_ds_downwind")?,
                            ))
                        }
                        "d2_ds2" => return Ok(SysExpr::D2ds2(self.expect_state_arg("d2_ds2")?)),
                        _ => {}
                    }
                    self.expect_sym('(')?;
                    let mut args = Vec::new();
                    if !self.is_sym(')') {
                        loop {
                            args.push(self.parse_sum()?);
                            if self.is_sym(',') {
                                self.next();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect_sym(')')?;
                    self.resolve_call(&name, args)
                } else {
                    self.resolve_ident(&name)
                }
            }
            other => Err(format!(
                "expected number, identifier, or '(', found {other:?}"
            )),
        }
    }

    fn resolve_call(&self, name: &str, mut args: Vec<SysExpr>) -> Result<SysExpr, String> {
        let one = |args: &mut Vec<SysExpr>, fn_name: &str| -> Result<Box<SysExpr>, String> {
            if args.len() != 1 {
                return Err(format!("{fn_name} takes 1 argument, given {}", args.len()));
            }
            Ok(Box::new(args.pop().unwrap()))
        };
        match name {
            "sin" => Ok(SysExpr::Fn1(SysFn1::Sin, one(&mut args, name)?)),
            "cos" => Ok(SysExpr::Fn1(SysFn1::Cos, one(&mut args, name)?)),
            "tan" => Ok(SysExpr::Fn1(SysFn1::Tan, one(&mut args, name)?)),
            "tanh" => Ok(SysExpr::Fn1(SysFn1::Tanh, one(&mut args, name)?)),
            "exp" => Ok(SysExpr::Fn1(SysFn1::Exp, one(&mut args, name)?)),
            "ln" | "log" => Ok(SysExpr::Fn1(SysFn1::Ln, one(&mut args, name)?)),
            "sqrt" => Ok(SysExpr::Fn1(SysFn1::Sqrt, one(&mut args, name)?)),
            "abs" => Ok(SysExpr::Fn1(SysFn1::Abs, one(&mut args, name)?)),
            "entry" => Ok(SysExpr::StationEntry(one(&mut args, name)?)),
            "exit" => Ok(SysExpr::StationExit(one(&mut args, name)?)),
            "mean" => Ok(SysExpr::StationMean(one(&mut args, name)?)),
            "integral" => Ok(SysExpr::StationIntegral(one(&mut args, name)?)),
            "min" | "max" | "pow" => {
                if args.len() != 2 {
                    return Err(format!("{name} takes 2 arguments, given {}", args.len()));
                }
                let b = Box::new(args.pop().unwrap());
                let a = Box::new(args.pop().unwrap());
                let f = match name {
                    "min" => SysFn2::Min,
                    "max" => SysFn2::Max,
                    _ => SysFn2::Pow,
                };
                Ok(SysExpr::Fn2(f, a, b))
            }
            "clamp" => {
                if args.len() != 3 {
                    return Err(format!("clamp takes 3 arguments, given {}", args.len()));
                }
                let hi = Box::new(args.pop().unwrap());
                let lo = Box::new(args.pop().unwrap());
                let x = Box::new(args.pop().unwrap());
                Ok(SysExpr::Clamp(x, lo, hi))
            }
            _ => Err(format!("unknown function '{name}'")),
        }
    }

    fn resolve_ident(&self, name: &str) -> Result<SysExpr, String> {
        match name {
            "pi" => return Ok(SysExpr::Const(PI)),
            "e" => return Ok(SysExpr::Const(EULER)),
            "s" => return Ok(SysExpr::CoordS),
            "L" => return Ok(SysExpr::SpanLen),
            "ds" => return Ok(SysExpr::GridStep),
            _ => {}
        }
        if self.scope.allow_states {
            if let Some(a) = self.scope.aux.iter().position(|ax| ax.name == name) {
                return Ok(SysExpr::Aux(a));
            }
            if let Some(f) = self.scope.states.iter().position(|st| st.name == name) {
                return Ok(SysExpr::State(f));
            }
            if let Some(stem) = name.strip_suffix("_prev") {
                if let Some(f) = self.scope.states.iter().position(|st| st.name == stem) {
                    return Ok(SysExpr::StatePrev(f));
                }
            }
        }
        if let Some(k) = self.scope.designs.iter().position(|d| d.name == name) {
            return Ok(SysExpr::Design(k));
        }
        if let Some(l) = self.scope.epistemics.iter().position(|t| t.name == name) {
            return Ok(SysExpr::Epistemic(l));
        }
        if let Some(idx) = self.scope.doc_lets.iter().position(|(n, _)| n == name) {
            return Ok(SysExpr::DocLet(idx));
        }
        Err(format!("unknown variable '{name}'"))
    }
}

fn compile_expr_str(src: &str, scope: &CompileScope<'_>) -> Result<SysExpr, String> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return Err("empty expression".into());
    }
    let toks = tokenize_sys_expr(trimmed)?;
    if toks.is_empty() {
        return Err("empty expression".into());
    }
    let mut parser = ExprParser {
        toks: &toks,
        pos: 0,
        scope,
    };
    let ex = parser.parse_sum()?;
    if parser.pos < parser.toks.len() {
        return Err(format!(
            "unexpected trailing token {:?}",
            parser.toks[parser.pos]
        ));
    }
    Ok(ex)
}

// =============================================================================================
// Coupled State -> 3D CAD & Spatial Boundary-Condition Projection
// =============================================================================================

/// Synchronise a solved `(state, design_p, epistemic_theta)` point of `doc.system_block` back
/// onto `doc`:
/// 1. Updates `system_block.designs[k].nominal = design_p[k]` and propagates any matching `let`
///    parameters through [`apply_doc_command`] (`DocCommand::SetLet`), re-evaluating consumed
///    dimensions and parametric 2D sketches (`sync_sketch_features`).
/// 2. Evaluates each [`DeclCadBinding`] across the 0D/1D stations and writes scalar values or
///    `lerp(<axis>, s_0, v_0, ..., s_{N-1}, v_{N-1})` spatial fields onto `let`s and 3D BCs.
pub fn sync_system_bindings_to_doc(
    doc: &mut Document,
    state: &[f64],
    design_p: &[f64],
    epistemic_theta: &[f64],
) -> Result<(), String> {
    sync_system_bindings_impl(doc, state, design_p, epistemic_theta, false)
}

/// Exact IEEE-754 variant of [`sync_system_bindings_to_doc`] for in-memory solver and sensitivity
/// evaluation (`SpatialFn::format_lerp_exact`).
pub fn sync_system_bindings_to_doc_exact(
    doc: &mut Document,
    state: &[f64],
    design_p: &[f64],
    epistemic_theta: &[f64],
) -> Result<(), String> {
    sync_system_bindings_impl(doc, state, design_p, epistemic_theta, true)
}

fn sync_system_bindings_impl(
    doc: &mut Document,
    state: &[f64],
    design_p: &[f64],
    epistemic_theta: &[f64],
    exact_lerp: bool,
) -> Result<(), String> {
    let sys_snapshot = doc
        .system_block
        .clone()
        .ok_or_else(|| "document has no declarative 'system' block".to_string())?;

    if design_p.len() != sys_snapshot.designs.len() {
        return Err(format!(
            "design_p length {} != system '{}' designs length {}",
            design_p.len(),
            sys_snapshot.id,
            sys_snapshot.designs.len()
        ));
    }
    if epistemic_theta.len() != sys_snapshot.epistemics.len() {
        return Err(format!(
            "epistemic_theta length {} != system '{}' epistemics length {}",
            epistemic_theta.len(),
            sys_snapshot.id,
            sys_snapshot.epistemics.len()
        ));
    }

    // 1. Update `system_block.designs` nominals and matching document `let`s
    if let Some(ref mut sb) = doc.system_block {
        for (k, &pv) in design_p.iter().enumerate() {
            sb.designs[k].nominal = pv;
        }
    }
    for (k, dp) in sys_snapshot.designs.iter().enumerate() {
        if doc.lets.iter().any(|l| l.name == dp.name) {
            apply_doc_command(
                doc,
                DocCommand::SetLet {
                    name: dp.name.clone(),
                    value: design_p[k],
                    expr: None,
                },
            )?;
        }
    }

    // 2. Re-compile with updated `doc.lets` and evaluate `bindings`
    let sys_now = doc.system_block.clone().unwrap();
    let let_vals: Vec<(String, f64)> = doc.lets.iter().map(|l| (l.name.clone(), l.value)).collect();
    let compiled = CompiledSystem::compile(&sys_now, &let_vals).map_err(|errs| errs.join("; "))?;
    if state.len() != compiled.num_dofs() {
        return Err(format!(
            "state length {} != compiled num_dofs {}",
            state.len(),
            compiled.num_dofs()
        ));
    }

    let bound_profiles = compiled.eval_bindings(state, design_p, epistemic_theta);
    let coord = sys_now.axis.to_spatial_coord();

    for (b, vals) in sys_now.bindings.iter().zip(bound_profiles.iter()) {
        if !vals.iter().all(|v| v.is_finite()) {
            return Err(format!(
                "System '{}' bind '{}' ({}) evaluated to a non-finite value",
                sys_now.id,
                b.target,
                b.attr.name()
            ));
        }
        let mean = vals.iter().sum::<f64>() / (vals.len().max(1) as f64);
        let min_v = vals.iter().copied().fold(f64::INFINITY, f64::min);
        let max_v = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let varies = compiled.nodes >= 2 && (max_v - min_v) > 1e-11;
        let lerp_expr = if varies {
            let samples: Vec<(f64, f64)> = compiled
                .stations
                .iter()
                .copied()
                .zip(vals.iter().copied())
                .collect();
            if exact_lerp {
                SpatialFn::format_lerp_exact(coord, &samples)
            } else {
                SpatialFn::format_lerp(coord, &samples)
            }
        } else {
            None
        };

        match b.attr {
            DeclBindAttr::Let => {
                apply_doc_command(
                    doc,
                    DocCommand::SetLet {
                        name: b.target.clone(),
                        value: mean,
                        expr: None,
                    },
                )?;
            }
            DeclBindAttr::Temp => {
                let t = doc
                    .temperatures
                    .iter_mut()
                    .find(|t| t.id == b.target)
                    .ok_or_else(|| format!("bind: missing temperature '{}'", b.target))?;
                t.celsius = mean;
                t.expr = lerp_expr;
            }
            DeclBindAttr::H => {
                if min_v < 0.0 {
                    return Err(format!(
                        "System '{}' bind '{}' (h) produced negative film coefficient ({min_v})",
                        sys_now.id, b.target
                    ));
                }
                let c = doc
                    .convections
                    .iter_mut()
                    .find(|c| c.id == b.target)
                    .ok_or_else(|| format!("bind: missing convection '{}'", b.target))?;
                c.h = mean;
                c.h_expr = lerp_expr;
            }
            DeclBindAttr::Tinf => {
                let c = doc
                    .convections
                    .iter_mut()
                    .find(|c| c.id == b.target)
                    .ok_or_else(|| format!("bind: missing convection '{}'", b.target))?;
                c.t_inf = mean;
                c.t_inf_expr = lerp_expr;
            }
            DeclBindAttr::Q => {
                let hf = doc
                    .heat_fluxes
                    .iter_mut()
                    .find(|hf| hf.id == b.target)
                    .ok_or_else(|| format!("bind: missing heatflux '{}'", b.target))?;
                hf.q = mean;
                hf.expr = lerp_expr;
            }
            DeclBindAttr::Pressure => {
                let ld = doc
                    .loads
                    .iter_mut()
                    .find(|l| l.id == b.target)
                    .ok_or_else(|| format!("bind: missing load '{}'", b.target))?;
                match &mut ld.kind {
                    LoadKind::Pressure { mpa, expr } => {
                        *mpa = mean;
                        *expr = lerp_expr;
                    }
                    _ => return Err(format!("bind: load '{}' is not a pressure load", b.target)),
                }
            }
            DeclBindAttr::Preload => {
                let ld = doc
                    .loads
                    .iter_mut()
                    .find(|l| l.id == b.target)
                    .ok_or_else(|| format!("bind: missing load '{}'", b.target))?;
                match &mut ld.kind {
                    LoadKind::Preload { newtons, .. } => {
                        *newtons = mean;
                    }
                    _ => return Err(format!("bind: load '{}' is not a preload load", b.target)),
                }
            }
            DeclBindAttr::ForceX | DeclBindAttr::ForceY | DeclBindAttr::ForceZ => {
                let comp = match b.attr {
                    DeclBindAttr::ForceX => 0,
                    DeclBindAttr::ForceY => 1,
                    DeclBindAttr::ForceZ => 2,
                    _ => unreachable!(),
                };
                let ld = doc
                    .loads
                    .iter_mut()
                    .find(|l| l.id == b.target)
                    .ok_or_else(|| format!("bind: missing load '{}'", b.target))?;
                match &mut ld.kind {
                    LoadKind::Force { vector } => {
                        vector[comp] = mean;
                    }
                    _ => return Err(format!("bind: load '{}' is not a force load", b.target)),
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dual_ad_jacobian_matches_central_fd_to_1e_9_on_coupled_1d_nonlinear_system() {
        let sys = DeclarativeSystem {
            id: "test_1d_rad_conv".into(),
            title: "1D Nonlinear Radiative-Convective Heated Pipe".into(),
            sector: "Thermal-Fluids".into(),
            nodes: 11,
            span: [0.0, 100.0],
            axis: DeclAxis::Z,
            designs: vec![
                DeclDesignParam {
                    name: "mdot".into(),
                    nominal: 0.06,
                    min: 0.02,
                    max: 0.20,
                    unit: "kg/s".into(),
                    symbol: "m_dot".into(),
                    category: DeclDesignCategory::Process,
                    cost_rate: 0.0,
                    description: "Coolant mass flow".into(),
                },
                DeclDesignParam {
                    name: "wall_t".into(),
                    nominal: 3.0,
                    min: 1.0,
                    max: 6.0,
                    unit: "mm".into(),
                    symbol: "t_w".into(),
                    category: DeclDesignCategory::Geometry,
                    cost_rate: 0.0,
                    description: "Wall thickness".into(),
                },
            ],
            epistemics: vec![DeclEpistemicParam {
                name: "h_mult".into(),
                nominal: 1.0,
                sigma: 0.1,
                min: 0.7,
                max: 1.3,
                unit: "-".into(),
                symbol: "c_h".into(),
                mechanism: "Correlation scatter".into(),
            }],
            states: vec![
                DeclStateField {
                    name: "T_f".into(),
                    init_expr: "25 + 0.2 * s".into(),
                    scale: 100.0,
                    unit: "C".into(),
                    symbol: "T_f".into(),
                },
                DeclStateField {
                    name: "T_w".into(),
                    init_expr: "80 + 0.3 * s".into(),
                    scale: 100.0,
                    unit: "C".into(),
                    symbol: "T_w".into(),
                },
            ],
            aux: vec![
                DeclAuxVar {
                    name: "h_eff".into(),
                    expr: "0.02 * h_mult * (mdot / 0.06)^0.8 * (1 + 0.001 * T_f)".into(),
                },
                DeclAuxVar {
                    name: "q_rad".into(),
                    expr: "1e-9 * ((T_w + 273.15)^4 - 300^4)".into(),
                },
            ],
            residuals: vec![
                DeclResidual {
                    state: "T_f".into(),
                    eq: "4180 * mdot * d_ds_upwind(T_f) - pi * 12 * h_eff * (T_w - T_f)".into(),
                    bc_left: Some("T_f - 25".into()),
                    bc_right: None,
                    transient: 1.0,
                },
                DeclResidual {
                    state: "T_w".into(),
                    eq: "-18 * wall_t * d2_ds2(T_w) + h_eff * (T_w - T_f) + q_rad - 0.45 * (1 + 0.3 * sin(pi * s / L))".into(),
                    bc_left: Some("d_ds(T_w)".into()),
                    bc_right: Some("d_ds(T_w)".into()),
                    transient: 1.0,
                },
            ],
            objectives: vec![DeclObjective {
                name: "peak_wall".into(),
                expr: "T_w".into(),
                reduce: DeclReduce::Max,
                kind: DeclObjKind::Upper { limit: 150.0 },
                unit: "C".into(),
                symbol: "T_max".into(),
                description: "Peak wall temperature".into(),
            }],
            audits: vec![],
            bindings: vec![],
            scenarios: vec![],
        };

        let compiled = CompiledSystem::compile(&sys, &[]).expect("compile 1D system");
        let p = vec![0.06, 3.0];
        let th = vec![1.0];
        let w = compiled.eval_initial_state(&p, &th);
        let w_prev: Vec<f64> = w.iter().map(|v| v - 1.5).collect();
        let inv_dt = 0.25;

        let (_, triplets, cross_nnz) =
            compiled.assemble_jacobian_triplets(&w, &w_prev, inv_dt, &p, &th);
        assert!(
            cross_nnz > 0,
            "expected cross-physics couplings between T_f and T_w"
        );

        let ndof = compiled.num_dofs();
        // Optimal central-FD step balances O(eps^2) truncation with O(u_mach / eps) roundoff:
        // eps = cbrt(3 * u_mach) ≈ 1e-5 gives ~1e-10 FD accuracy against machine-exact Dual AD.
        let eps = 1e-5;
        let dw: Vec<f64> = (0..ndof).map(|k| ((k as f64 + 1.0) * 1.37).cos()).collect();
        let w_plus: Vec<f64> = w.iter().zip(&dw).map(|(x, d)| x + eps * d).collect();
        let w_minus: Vec<f64> = w.iter().zip(&dw).map(|(x, d)| x - eps * d).collect();
        let r_plus = compiled.eval_residual(&w_plus, &w_prev, inv_dt, &p, &th);
        let r_minus = compiled.eval_residual(&w_minus, &w_prev, inv_dt, &p, &th);

        let mut j_dw = vec![0.0; ndof];
        for &(row, col, val) in &triplets {
            j_dw[row] += val * dw[col];
        }
        let mut num_sq = 0.0;
        let mut den_sq = 0.0;
        for k in 0..ndof {
            let fd = (r_plus[k] - r_minus[k]) / (2.0 * eps);
            num_sq += (j_dw[k] - fd).powi(2);
            den_sq += fd.powi(2);
        }
        let rel_err = (num_sq / den_sq).sqrt();
        assert!(
            rel_err < 1e-9,
            "Dual-AD Jacobian directional error {rel_err:e} >= 1e-9"
        );
    }

    #[test]
    fn fsm_system_block_round_trips_canonically_and_projects_1d_lerp_bcs() {
        let src = "\
fieldsmith 1
name \"Cooled Channel Duct\"
material ss316l
let wall_t = 3
let length_z = 80
cylinder outer_wall r=8 + wall_t h=length_z
cylinder inner_bore r=8 h=length_z + 2 subtract
region bore_surf = inner_bore.side
region outer_surf = outer_wall.side
region base_cap = outer_wall.bottom
support fix_base base_cap fixed=xyz
convection bore_cool bore_surf h=0.015 tinf=25 case=\"Thermal\"
heatflux outer_flux outer_surf q=0.25 case=\"Thermal\"

system channel_cooler title=\"1D Convective Channel Cooler\" sector=\"Thermal-Fluids\" nodes=5 span=(-40, 40) {
  design mdot nominal=0.08 min=0.02 max=0.25 unit=\"kg/s\" symbol=\"m_dot\" desc=\"Coolant mass flow\"
  design wall_t nominal=3 min=1.5 max=6 unit=\"mm\" symbol=\"t_w\" category=geometry desc=\"Wall thickness\"
  epistemic h_mult nominal=1 sigma=0.12 min=0.6 max=1.5 unit=\"-\" symbol=\"c_h\" mechanism=\"Correlation scatter\"
  state T_f init=\"25\" scale=100 unit=\"C\" symbol=\"T_f\"
  state T_w init=\"70\" scale=100 unit=\"C\" symbol=\"T_w\"
  aux h_eff expr=\"0.018 * h_mult * (mdot / 0.08)^0.8\"
  aux q_ext expr=\"0.25 * (1 + 0.2 * cos(pi * s / L))\"
  residual T_f eq=\"4180 * mdot * d_ds_upwind(T_f) - pi * 16 * h_eff * (T_w - T_f)\" bc_left=\"T_f - 25\" transient=1
  residual T_w eq=\"-16 * wall_t * d2_ds2(T_w) + h_eff * (T_w - T_f) - q_ext\" bc_left=\"d_ds(T_w)\" bc_right=\"d_ds(T_w)\" transient=1
  objective peak_wall expr=\"T_w\" reduce=max kind=upper limit=130 unit=\"C\" symbol=\"T_w,max\" desc=\"Peak wall temperature\"
  objective exit_temp expr=\"T_f\" reduce=exit kind=upper limit=80 unit=\"C\" symbol=\"T_f,out\"
  audit heat_balance inflow=\"integral(pi * 16 * q_ext)\" outflow=\"4180 * mdot * (exit(T_f) - entry(T_f))\" tol=0.05 unit=\"W\"
  bind bore_cool attr=h expr=\"h_eff\"
  bind bore_cool attr=tinf expr=\"T_f\"
  bind outer_flux attr=q expr=\"q_ext\"
  scenario \"Low Flow\" p=[0.04, 2.5] desc=\"Under-cooled baseline\"
}
solve tref=20
";
        let mut doc = crate::fsm::parse(src).expect("parse .fsm with system block");
        let errs = doc.validate();
        assert!(
            errs.is_empty(),
            "expected 0 validation errors, got {errs:?}"
        );

        let printed_1 = crate::fsm::print(&doc);
        let reparsed = crate::fsm::parse(&printed_1).expect("reparse printed .fsm");
        let printed_2 = crate::fsm::print(&reparsed);
        assert_eq!(
            printed_1, printed_2,
            "fsm::print must be canonical/idempotent"
        );
        assert_eq!(doc.system_block, reparsed.system_block);

        // Project a synthetic 1D profile onto `doc` via `sync_system_bindings_to_doc`
        let fake_state = vec![
            25.0, 62.0, // s = -40
            30.0, 68.0, // s = -20
            36.0, 74.0, // s = 0
            41.0, 72.0, // s = 20
            45.0, 67.0, // s = 40
        ];
        sync_system_bindings_to_doc(&mut doc, &fake_state, &[0.12, 4.5], &[1.0])
            .expect("sync bindings");
        let wall_let = doc.lets.iter().find(|l| l.name == "wall_t").unwrap();
        assert!((wall_let.value - 4.5).abs() < 1e-12);
        let conv = doc
            .convections
            .iter()
            .find(|c| c.id == "bore_cool")
            .unwrap();
        assert!(
            conv.t_inf_expr
                .as_deref()
                .is_some_and(|e| e.starts_with("lerp(z, -40, 25")),
            "expected lerp(z, ...) on tinf_expr, got {:?}",
            conv.t_inf_expr
        );
        let hf = doc
            .heat_fluxes
            .iter()
            .find(|f| f.id == "outer_flux")
            .unwrap();
        assert!(
            hf.expr
                .as_deref()
                .is_some_and(|e| e.starts_with("lerp(z, -40,")),
            "expected lerp(z, ...) on heatflux expr, got {:?}",
            hf.expr
        );
        assert!(
            doc.validate().is_empty(),
            "synced doc must validate cleanly"
        );
    }
}
