# T8 — Coupled Multi-Physics Systems: When Fluid Flow, Heat, Chemistry & Structure Interact

> **Category**: Multiphysics Case Study · **Level**: Flagship Multi-Physics · **Verification**: Lesson 8 · Advanced · Coupled Multi-Physics
> **Repository Path**: `docs/case-studies/T8-float-bath-cvd-coater.md` · **Starter Model**: `t8-uk5-float-bath-twin.fsm` · **Verification Reports**: `docs/verification/V52.md`

In real industrial machines — like a molten-tin float-glass bath or a liquid-hydrogen rocket nozzle — fluid flow, heat transfer, and structural bending all affect one another simultaneously.

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "T8 — Multi-Physics Float-Bath & Coater Twin"
material ss316l
custommaterial sic_refractory name="Alumino-Silicate / SiC Refractory" e=95000 nu=0.20 yield=140 density=2.65 wall=1.5 a=0.0000046 source="Fused-cast float-bath refractory"
custommaterial molten_tin name="Molten Tin Bath Sn(l)" e=1200 nu=0.45 yield=18 density=6.50 wall=0.8 a=0.0000909 source="Pilkington tin2fe liquid metal (Ron Smith JFM 1990)"
custommaterial soda_lime_glass name="Soda-Lime Float Glass (VFT Melt)" e=72000 nu=0.22 yield=65 density=2.50 wall=0.8 a=0.000009 source="Pilkington float2fe VFT melt log10 eta = -2.65 + 4210/(T-264)"
custommaterial h2_n2_gas name="Reducing H2/N2 Bath Atmosphere" e=100 nu=0.30 yield=5 density=0.15 wall=0.8 a=0.000015 source="Forming-gas headspace + SnO/H2 Navier-slip cushion"

let bath_len = 216
let bath_w = 52
let sill_step = 8
let beam_x = 6
let h_sill = 65
let u_pull = 0.16
let i_motor = 260
let p_h2 = 4.2
let q_beam = 34

box support_pedestal size=(180, 40, 12) at=(0, 0, 6)
color "#475569" on support_pedestal
box refractory_casing size=(bath_len, bath_w, 16) at=(0, 0, 14)
box tin_pool_cavity size=(200, 42, 10) at=(0, 0, 18) subtract
material sic_refractory on refractory_casing
color "#6b665e" on refractory_casing
box tin_bath_pool size=(198, 40, 4.5) at=(0, 0, 15.2)
material molten_tin on tin_bath_pool
color "#64748b" on tin_bath_pool
box transition_sill size=(36, 42, sill_step) at=(10, 0, 19)
material sic_refractory on transition_sill
color "#8c6239" on transition_sill
box ribbon_hot_spread size=(78, 38, 6) at=(-58, 0, 17)
material soda_lime_glass on ribbon_hot_spread
color "#f59e0b" on ribbon_hot_spread
box glass_ribbon size=(142, 26, 5) at=(28, 0, 16.5)
material soda_lime_glass on glass_ribbon
color "#38bdf8" on glass_ribbon
box cvd_coater_beam size=(14, bath_w + 8, 9) at=(beam_x, 0, 24.5)
color "#e05a47" on cvd_coater_beam

region casing_base = support_pedestal.nz
region coater_slot = cvd_coater_beam.pz
region hot_spout = refractory_casing.nx
support fix_base casing_base fixed=xyz
load coater_weight coater_slot force=(0, 0, -28000) case="Coater Thermal & Gravity"
temperature spout_hot hot_spout temp=310
temperature coater_water coater_slot temp=46
solve tref=20
```

## 2. Step-by-Step Guided Walkthrough

### 1. Assigning Molten Tin & VFT Viscous Glass Continuum Materials in Shape (1) *(Lens: `SHAPE`)*

Click "tin_bath_pool" or "glass_ribbon" in the left Shape (1) tree and inspect the right-hand Inspector: notice the Material dropdown (`[data-testid="node-material"]`) and the "Continuum Phase" card (`[data-testid="node-continuum-card"]`). "tin_bath_pool" is assigned "Molten Tin Bath Sn(l)" (Liquid Metal · VMS + MHD, ρ₀ = 6500 kg/m³, β_Sn = 9.09×10⁻⁵ K⁻¹), "glass_ribbon" is assigned "Soda-Lime Float Glass (VFT Melt)" (log₁₀ η(T) = −2.65 + 4210/(T − 264 °C)), and "refractory_casing" is assigned "Alumino-Silicate / SiC Refractory".

- **Expected Check**: Selecting any shape node in Shape (1) shows its Continuum Phase badge, constitutive law, and physical constants in the Inspector.

### 2. Boundary Conditions & Automatic Field Equation Activation in Setup (2) & Theory (6) *(Lens: `SETUP`)*

Tap "Go to Setup →" (tab 2). Scroll to the "Multiphysics BCs & Couplings" section (`[data-testid="setup-multiphysics-section"]`): because the assembly contains Molten Tin and VFT Viscous Glass alongside thermal/mechanical boundary conditions (`spout_hot`, `coater_water`, `h_sill = 65`, `u_pull = 0.16`, `i_motor = 260`), the "Auto-Activated PDEs" card (`[data-testid="setup-active-equations"]`) shows that 3D P2 Linear Elasticity, Fourier Conduction, VFT Viscous Glass Sheet, and Turbulent Tin VMS + Lorentz MHD are all active! Click "Theory (6) →" (`[data-testid="setup-open-theory"]`) to inspect the live Applied Mathematics specification and export a `.md` or `.html` report.

- **Expected Check**: Setup (2) displays the active multiphysics boundary parameters and auto-activated PDEs, and Theory (6) documents the complete strong and weak variational equations.

### 3. Adjoint Sensitivities & Multi-Domain Solve in Results (3) & Coupled Systems *(Lens: `SYSTEMS`)*

Tap "Go to Coupled Systems →" to open the interactive Multi-Physics Workbench (or switch to Results (3) to inspect the 3D multi-material FEA mesh and transient video recorder). Click "Adjoint Step (−dΦ/dp)" in the top bar to nudge the 7 design dials using exact sparse Adjoint gradients.

- **Expected Check**: Clicking "Adjoint Step" updates the design levers using exact gradients and syncs the updated 3D `.fsm` geometry.

## 3. Theoretical Manual — Chapter 8 — Coupled Differential Equations, Adjoint Gradients & Uncertainty Bands

How can one solver engine handle both glass manufacturing and rocket propulsion, compute exact design gradients in milliseconds, and quantify real-world uncertainty?

### 8.1 Solving Coupled Equations Together & The Adjoint Shortcut

```text
Residual Balance: R(W; p) = 0,   Jacobian Matrix: J = ∂R / ∂W
Adjoint Equation: Jᵀ λ = (∂Φ / ∂W)ᵀ   ⇒   Exact Gradient: dΦ/dp = ∂Φ/∂p − λᵀ (∂R/∂p)
```

Let W be the vector of all physical unknowns across the machine (velocities, temperatures, chemical concentrations) and p be the design dials we can adjust. Solving R(W; p) = 0 by Newton’s method uses the Jacobian matrix J (how every equation reacts to every state variable). Once J is factored, solving one linear system with its transpose (Jᵀ λ = ∂Φ/∂Wᵀ) gives the sensitivity dΦ/dp of a performance metric Φ with respect to all design dials p at once — whether there are 5 dials or 5,000.

### 8.2 Propagating Real-World Uncertainty (p10 / p50 / p90 Confidence Bands)

```text
Variance: σ²(Φ) = ∑ (dΦ / dθ_l)² · σ²(θ_l),   p90 Bound = Φ_nominal + 1.2816 · σ(Φ)
```

In real engineering, some physical constants θ_l (such as furnace roof insulation aging or rough-wall cooling turbulence) are not known to 4 decimal places. By differentiating the coupled physics with respect to those uncertain parameters θ_l, Fieldsmith computes the 10th, 50th (median), and 90th percentile (p90 worst-case) bands in real time so you can design with a safety margin against real-world variability.

## 4. Reproducible Automation (MCP / REST API & CLI)

### List Pluggable Multi-Physics Engineering Packs via API (`list_coupled_systems`)

Query all registered coupled-physics domain models and their design dials and objectives.

```json
{
  "tool": "list_coupled_systems",
  "arguments": {}
}
```

### Run 2 Adjoint Optimisation Steps on the Float-Bath via API (`optimise_coupled_system`)

Automatically step the float-bath design dials along their Adjoint gradients and sync the 3D CAD model.

```json
{
  "tool": "optimise_coupled_system",
  "arguments": {
    "system_id": "pilkington_float_bath_cvd_coater_v1",
    "steps": 2,
    "sync_cad": true
  }
}
```

### Optimise the Supercritical-H₂ Rocket Nozzle & Load 3D Chamber via API (`optimise_coupled_system`)

Switch to the rocket nozzle pack, run 2 Adjoint optimisation steps, and project the 3D rocket chamber into the viewport.

```json
{
  "tool": "optimise_coupled_system",
  "arguments": {
    "system_id": "regen_nozzle_h2_v1",
    "steps": 2,
    "sync_cad": true
  }
}
```
