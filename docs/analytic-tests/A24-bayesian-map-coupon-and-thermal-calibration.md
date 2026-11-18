# A24 — Analytic Benchmark A24: Conjugate Linear-Gaussian & Nonlinear Robin Thermal Bayesian MAP Calibration

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Verified by A24 & V63 · Exact Conjugate Gaussian MAP + Laplace Posterior σ_post + Nonlinear Robin Inverse
> **Repository Path**: `docs/analytic-tests/A24-bayesian-map-coupon-and-thermal-calibration.md` · **Starter Model**: `a24-bayesian-map-coupon-and-thermal-calibration.fsm` · **Verification Reports**: `docs/verification/V1.md`, `docs/verification/V20.md`, `docs/verification/V63.md`

Verify 3D adjoint Gauss–Newton / Levenberg–Marquardt Bayesian Maximum-A-Posteriori (MAP) inverse calibration and Laplace posterior covariance (`Σ_post = diag(σ_0) (I + Aᵀ A)⁻¹ diag(σ_0)`) against exact closed-form conjugate Gaussian and 1D Robin conduction analytical solutions on a 3D P2 uniaxial/thermal coupon.

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| Uniaxial Prior Extension u(f_0 = 8400 N) | `u_0 = f_0 L / (E A) = 8400 × 60 / (70000 × 72)` | **0.100000 mm** | **0.100000 mm** | 0.00% | ≤ 0.2% |
| Conjugate Gaussian Posterior Force Mean μ_post(f_pull) | `μ_post = f_0 + σ_0 · A_11 r_0 / (1 + A_11²) = 8400 + 2100 × (10/13)` | **10015.385 N** | **10015.385 N** | 0.00% | ≤ 0.2% |
| Conjugate Gaussian Posterior Force Std σ_post(f_pull) | `σ_post = σ_0 / √(1 + A_11²) = 2100 / √26` | **411.8439 N** | **411.8439 N** | 0.00% | ≤ 0.2% |
| Prior 1D Robin Hot-End Temperature T(h_0 = 0.0050) | `T_0 = T_inf + q L / k + q / h_0 = 25 + 4.5 + 15.0` | **44.5000 °C** | **44.5000 °C** | 0.00% | ≤ 0.2% |
| Nonlinear Robin MAP Conductance h_MAP & Laplace σ_post(h) | `dJ_MAP/dh = 0  (T(h_MAP) = 64.980 °C, σ_post = 2.98e-5)` | **0.002114 ± 0.000030** | **0.002114 ± 0.000030** | 0.01% | ≤ 1.0% |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
name "A24 — Conjugate Gaussian & Nonlinear Robin MAP Calibration"
custommaterial cal_alloy name="Calibrated Test Coupon Alloy" e=70000 nu=0.0 yield=350 density=2.70 wall=0.8 a=0.0
material cal_alloy

let f_pull = 8400.0

box coupon size=(60, 12, 6) at=(30, 0, 0)

region mount_end = coupon.nx
region sensor_end = coupon.px

support mount_fix mount_end fixed=xyz
load axial_pull sensor_end force=(f_pull, 0, 0) case="AxialCal"
heatflux heater_in sensor_end q=0.075 case="ThermalCal"
convection cold_mount mount_end h=0.0050 tinf=25.0 case="ThermalCal"

solve size=3.0 tref=25

uncertainty "Actuator load prior" param="f_pull" std=2100.0 unit="N"
uncertainty "Mount thermal conductance prior" param="cold_mount.h" std=0.0015 unit="W/(mm^2*K)"

observe "Laser extensometer u_max" metric="max_displacement:AxialCal" measured=0.1200 std=0.0050 unit="mm"
observe "Hot-end K-type thermocouple" metric="max_temp:ThermalCal" measured=65.00 std=0.50 unit="C"

assert "Coupon physical mass" metric="mass_g" target=11.664 tol="0.5%" unit="g"
assert "Prior uniaxial extension u_0" metric="max_displacement:AxialCal" target=0.100000 tol="0.5%" unit="mm"
assert "Prior 1D Robin hot-end temperature T_0" metric="max_temp:ThermalCal" target=44.5000 tol="0.5%" unit="C"
assert "Prior 90th-percentile uniaxial extension p90(u)" metric="p90.max_displacement:AxialCal" target=0.132039 tol="1.0%" unit="mm"
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect Prior Uncertainties & Bench Sensor Observations in Results (3) *(Lens: `RESULTS`)*

Switch to **3. Results**. Under the prior (`f_pull = 8400 ± 2100 N`, `cold_mount.h = 0.0050 ± 0.0015 W/(mm²·K)`), the coupon predicts `u_max = 0.1000 mm` and `T_max = 44.50 °C`, whereas the authored bench readings (`observe`) report `0.1200 ± 0.0050 mm` and `65.00 ± 0.50 °C`.

- **Expected Check**: Clicking **Verify Asserts (4)** confirms the exact prior P2 linear-field predictions (`0.1000 mm`, `44.50 °C`, and `p90 = 0.1320 mm`) to machine precision.

### 2. Run Bayesian MAP Calibration & Verify Closed-Form Posterior *(Lens: `RESULTS`)*

Click **Calibrate MAP (2)** in the Results sensitivity & calibration card. Marquardt-damped Gauss–Newton converges in `6` iterations (`J_MAP: 848.5 → 2.160`), recovering the exact conjugate Gaussian posterior `f_pull = 10015.4 ± 411.8 N` (`−80.4%` σ) and nonlinear Robin posterior `cold_mount.h = 0.00211 ± 0.00003 W/(mm²·K)` (`−98.0%` σ).

- **Expected Check**: Clicking **Apply Posterior to .fsm** updates `let f_pull`, `cold_mount.h`, and both marginal `.fsm` `uncertainty` `std` entries in a single undoable transaction.

## 3. Theoretical Manual — Analytic Verification A24 — Conjugate Linear-Gaussian & Nonlinear Robin Bayesian MAP Calibration

On a prismatic coupon (`L = 60 mm`, `A = 12 × 6 = 72 mm²`) with `ν = 0` and insulated lateral faces, both uniaxial displacement `u_x(x) = (f_pull / (E A)) x` and steady conduction `T(x) = T_inf + q/h + (q/k) x` (`k = 1 W/(mm·K)`) are strictly linear in `x`. Quadratic P2 tetrahedra represent both fields with zero spatial discretization error, isolating the inverse solver and Laplace covariance math to machine precision.

### A24.1 Exact Conjugate Gaussian Posterior for Linear Force Calibration

```text
H_u = ∂u_max / ∂f_pull = L / (E A) = 60 / (70000 × 72) = 1 / 84000 mm/N
A_11 = (σ_0 / σ_y) H_u = (2100 / 0.005) / 84000 = 5,   Λ_z = 1 + A_11² = 26
μ_post = f_0 + σ_0 · A_11 ((y_u − u_0)/σ_y) / Λ_z = 8400 + 2100 × (20 / 26) = 10015.384615 N,   σ_post = 2100 / √26 = 411.843884 N
```

Because `u_max` is linear in `f_pull` and decoupled from `ThermalCal`, the standardized posterior precision `Λ_z = 26` is constant everywhere. One Gauss–Newton step reaches `μ_post = 10015.38462 N` and `σ_post = 411.84388 N`.

### A24.2 Nonlinear 1/h Robin Mount Conductance Inverse & Marquardt Damping

```text
T_max(h) = T_inf + q L / k + q / h = 29.5 + 0.075 / h,   H_T(h) = −q / h²
∂J_MAP / ∂h = ((T_max(h) − y_T) / σ_T) (H_T(h) / σ_T) + (h − h_0) / σ_h0² = 0   ⟹   h_MAP = 0.00211386 W/(mm²·K)
```

At `h_0 = 0.0050`, an undamped linear tangent step toward `y_T = 65 °C` overshoots `h = 0` (`Δh_GN = −0.0068`). Diagonal Marquardt damping (`G_jj ← G_jj (1 + λ)`) regularizes early steps across the `1/h` pole and converges quadratically to `h_MAP = 0.00211386` with Laplace `σ_post = 2.978e-5 W/(mm²·K)`.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Verify exact prior P2 extension, Robin temperature, and p90 quantile contracts

```json
{
  "tool": "verify",
  "arguments": {
    "dofs": 6000
  }
}
```

### Run pure-query Bayesian MAP calibration without mutating prior .fsm

```json
{
  "tool": "calibrate",
  "arguments": {
    "dofs": 6000,
    "apply": false
  }
}
```

### Clear prior-specific target contracts before applying posterior

```json
{
  "tool": "delete_assert",
  "arguments": {
    "index": 3
  }
}
```

### Clear prior temperature target contract

```json
{
  "tool": "delete_assert",
  "arguments": {
    "index": 2
  }
}
```

### Clear prior extension target contract

```json
{
  "tool": "delete_assert",
  "arguments": {
    "index": 1
  }
}
```

### Apply Bayesian MAP calibration to update posterior means & marginal sigmas

```json
{
  "tool": "calibrate",
  "arguments": {
    "dofs": 6000,
    "apply": true
  }
}
```

### Add posterior calibrated extension contract

```json
{
  "tool": "upsert_assert",
  "arguments": {
    "label": "Posterior MAP extension u_MAP",
    "metric": "max_displacement:AxialCal",
    "target": "0.119231",
    "tolerance": "0.5%",
    "unit": "mm"
  }
}
```

### Add posterior 90th-percentile hot-end temperature contract

```json
{
  "tool": "upsert_assert",
  "arguments": {
    "label": "Posterior p90 hot-end temperature",
    "metric": "p90.max_temp:ThermalCal",
    "target": "<= 65.8",
    "tolerance": "<= 65.8",
    "unit": "C"
  }
}
```

### Verify posterior calibrated .fsm contracts

```json
{
  "tool": "verify",
  "arguments": {
    "dofs": 6000
  }
}
```

---
*Generated from `examples/analytic/A24-bayesian-map-coupon-and-thermal-calibration.fsm` by `node scripts/gen-sheet-docs.mjs`.*
