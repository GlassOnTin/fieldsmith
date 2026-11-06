# A13 — Analytic Benchmark A13: Fastener Inclined-Plane Thread Torque Envelope [T_min, T_max] & Weibull Regression (V57)

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Verified by V57 · NASA/ECSS/VDI 2230 Fastener Standards · PASS (< 0.1%)
> **Repository Path**: `docs/analytic-tests/A13-fastener-thread-torque-and-weibull.md` · **Starter Model**: `a13-fastener-torque-weibull.fsm` · **Verification Reports**: `docs/verification/V53.md`

Verify Kellermann–Klein / VDI 2230 thread-plus-collar torque decomposition T = F [½ d₂ tan(ψ + ρ') + μ_b r_b] against NASA-STD-5020B, ECSS-E-ST-32-08C, and Bossard boss-tapping benchmarks plus closed-form Weibull MLE/OLS regression.

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| Kellermann–Klein / VDI 2230 Thread + Collar Torque Decomposition T(F) | `T = F [½ d₂ tan(ψ + ρ') + μ_b r_b],   tan ρ' = μ_t / cos(α/2),   tan ψ = P / (π d₂)` | **Exact Algebraic Identity (T_thread + T_collar = T_total)** | **Exact Match across all 7 Fixing Kinds** | 0.000% | ≤ 10⁻⁹ |
| Open-Standard Qualification Benchmarks (NASA-STD-5020B M6 / ECSS M4 / Bossard M3) | `F_proof = A_t (0.75 σ_y) = 14,472 N (A286 M6×1.0);   T_max > T_min Safe Window` | **4 / 4 Open Standards PASS (NASA, ECSS, VDI 2230, Bossard)** | **4 / 4 PASS (T_rec within [T_min, T_max])** | 0.000% | Exact |
| 2-Parameter Weibull Failure Distribution Linear Regression (β, η, B10, B01) | `ln(−ln(1 − F(T))) = β ln T − β ln η,   T_B10 = η (−ln 0.90)^{1/β}` | **R² > 0.98,  T_B01 < T_B10 < η** | **R² = 0.996,  Exact Quantile Ordering** | < 0.1% | R² ≥ 0.98 |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "A13 — Fastener Thread Torque & Preload Benchmark (V57)"
material pa12

box boss_plate size=(32, 24, 10) at=(0, 0, 0) round=1
cylinder pilot_hole r=1.6 h=8.5 at=(0, 0, 1.5) subtract
cylinder counterbore r=3.5 h=2 at=(0, 0, 8) subtract
cylinder screw_head r=3.2 h=2.2 at=(0, 0, 9.1)
material ti64 on screw_head

region base_mount = boss_plate.nz
region washer_seat = counterbore.bottom
support mount_fix base_mount fixed=xyz
load clamp_preload washer_seat preload=1250 axis=(0, 0, -1)
solve
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect the Drilled Boss & Washer Seat Preload in Setup (2) *(Lens: `SETUP`)*

Open Setup (2). A Ti-6Al-4V M4 fixing clamps a PA12 enclosure boss (`boss_plate`) with `preload=1250` N on the counterbore washer seat (`washer_seat`).

- **Expected Check**: The counterbore seat receives the downward clamping preload while the base is fixed.

### 2. Open the Fixing Torque & Weibull Workbench or Inspect 3D Boss Stress in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3) to inspect the 3D bearing and thread-root stress in the PA12 boss, or open the Fixings & Weibull Workbench in T14 to compare the NASA-STD-5020B, ECSS-E-ST-32-08C, VDI 2230, and Bossard self-tapping torque envelopes.

- **Expected Check**: 3D FEA confirms finite bearing compression under the washer seat and safe boss hoop stress.

## 3. Theoretical Manual — Analytic Verification A13 — Helical Inclined-Plane Thread Mechanics & Weibull Reliability (V57)

Derives the Kellermann–Klein torque-preload relationship on ISO 60° and self-tapping flank profiles, internal/external thread shear stripping areas (Alexander 1977), and Weibull B10/B01 quantile estimation.

### A13.1 Kellermann–Klein Thread Torque & Alexander Thread-Stripping Areas

```text
T_drive = F_preload [ ½ d₂ tan(ψ + ρ') + μ_b r_b ]
A_s,int = π d_major L_eff [ ½ + (1/√3)(d_major − d₂)/P ],   F_strip = A_s,int τ_u,substrate
```

Minimum assembly torque T_min is governed by slip/gasket sealing plus thread-forming work, while maximum torque T_max is the minimum of torsional/tensile bolt yield (von Mises combined σ_ax² + 3τ_t²), substrate thread shear stripping, and under-head bearing crush.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Insert M4 Fixing & Compute [T_min, T_max] Envelope via API (`insert_fixing`)

Call insert_fixing to auto-tap the host and return the VDI 2230 / NASA-STD-5020B torque window.

```json
{
  "tool": "insert_fixing",
  "arguments": {
    "kind": "self_tapping_screw",
    "size": "M4",
    "engagement_mm": 8,
    "host_id": "boss_plate",
    "fastener_material": "ti64"
  }
}
```
