# T14 — Engineering Fixings: Minimal & Maximal Torque, Weibull/Creep Spreadsheet & JWST Qualification

> **Category**: Guided Tutorial · **Level**: Flagship Multi-Physics · **Verification**: Verified by V57 (Motosh Sliding Surface + Weibull Regression R² > 0.999 + 72-DOF Adjoint)
> **Repository Path**: `docs/tutorials/T14-fastener-torque-weibull-preload.md` · **Starter Model**: `t14-self-tapping-fixing-torque.fsm` · **Verification Reports**: `docs/verification/V53.md`

Built-in catalogue of self-tapping screws, hex head screws, socket cap bolts, washers, nuts, Nyloc nuts, and cantilever hook-clip plastic boxes — with automatic host tapping, Motosh sliding-surface torque T_min, and material-adaptive T_max (Plastic Creep vs Metal Yield vs JWST Weibull Fracture + 3K Woven CFRP).

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "T14 Self-Tapping Screw & Hook-Clip Fixing Qualification"
material petg

let screw_d = 4.0
let screw_pitch = 0.70
let pilot_r = 1.60
let boss_r = 5.0
let eng_len = 8.0
let torque_nm = 1.38
let preload_n = 795.0

box instrument_flange size=(36, 26, 6) at=(0, 0, -3) round=1.2
cylinder receiving_boss r=boss_r h=eng_len at=(0, 0, 2) round=0.4 blend=0.6
cylinder seat_spotface r=4.5 h=0.8 at=(0, 0, 5.8) subtract
cylinder self_tap_bore r=pilot_r h=eng_len at=(0, 0, 2.2) subtract
torus thread_bite_ring major=1.80 minor=0.35 at=(0, 0, 3.2) subtract
cylinder screw_head r=3.5 h=3.8 at=(0, 0, 7.8) round=0.35
material ti64 on screw_head
color "#94A3B8" on screw_head

box snap_catch_pocket size=(3.0, 6.8, 3.0) at=(14.5, 0, -1.2) round=0.2 subtract
box hook_clip_arm size=(1.8, 6.0, 10.0) at=(16.5, 0, 2.0) round=0.35 blend=0.5
color "#38BDF8" on hook_clip_arm
box hook_clip_tip size=(2.8, 6.0, 2.4) at=(15.9, 0, 6.2) round=0.3 blend=0.25
color "#0284C7" on hook_clip_tip

region base_mount = instrument_flange.nz
region screw_clamp_seat = screw_head.top + seat_spotface.bottom + self_tap_bore.side
support mount_fix base_mount fixed=xyz
load screw_preload screw_clamp_seat preload=preload_n
```

## 2. Step-by-Step Guided Walkthrough

### 1. Insert Engineering Fixings & Auto-Tap the Fixed Object (Shape 1) *(Lens: `SHAPE`)*

In the Shapes tree on the left, click "🔩 Fixings…" (or click + and choose any fixing). Browse the 7 built-in engineering fixings: Self-Tapping Screw, Hex Head Screw (ISO 4017), Hex Cap Bolt (ISO 4762), Structural Washer, Hex Nut, Nyloc Lock Nut (with PA12 insert), and Cantilever Hook-Clip Plastic Box Snap-Fit. Clicking Insert automatically cuts the pilot bore, spotface/counterbore, thread-bite rings, hex nut trap, or snap-fit catch slot into the host object and configures the clamping preload.

- **Expected Check**: The host housing automatically gains subtracted pilot/tapped holes and counterbores alongside the 3D fixing hardware and let parameters.

### 2. Minimal Torque (T_min) from Sliding-Surface Friction & Vibration/Knock *(Lens: `SHAPE`)*

In the interactive Fixing Torque Envelope Lab above, adjust the Vibration / Knock RMS g-slider (1–45 g) and Thread Friction μ_t slider. Observe how Minimal Torque T_min is computed from the helical inclined-plane sliding surface (Motosh lead P/(2π) + flank friction μ_t r_t / cos α_n + underhead friction μ_h r_h) plus self-tapping thread-forming torque T_form to maintain Junker anti-slip clamping after viscoplastic boss creep η_creep and thermal ΔT.

- **Expected Check**: Raising vibration from 12 g to 28 g increases the required residual clamp force F_clamp,min and raises T_min accordingly.

### 3. Compare Plastic Boss (PETG/PA12) vs Metal Housing (AlSi10Mg) Maximal Torque (T_max) *(Lens: `SETUP`)*

Click "1. Plastic Boss (PETG/PA12)" and then "2. Metal Housing (AlSi10Mg)" in the workbench preset bar (or switch to 4. 3K Woven CFRP). Observe how Fieldsmith automatically selects the governing failure mode for the material regime: Viscoplastic Boss Hoop Creep / Radial Split in polymers (T_max ≈ 1.9 N·m), Internal Thread Shear / Core Tension–Torsion Yield in metals (T_max ≈ 4.6 N·m), or Tsai–Wu Interlaminar Peel in woven composites.

- **Expected Check**: T_max and the Governing Failure Mode readout automatically adapt to the selected substrate material.

### 4. Fit Failure Distributions in the Interactive Empirical Coupon Spreadsheet *(Lens: `RESULTS`)*

In the Empirical Failure Sample Probability Spreadsheet table above, inspect or edit the coupon rows (Applied Torque T, Peak Principal Stress σ₁, and Observed Failure Probability P_f %) or click "+ Add Sample Row". The live linear regression fits the 2-parameter Weibull distribution (slope m, characteristic scale σ₀, torque scale T₀, and R²) and updates the SVG Weibull plot and T_max.

- **Expected Check**: Editing any coupon cell or switching target reliability P_f (10⁻² down to 10⁻⁶) immediately updates the Weibull regression line and allowable torque T_max.

### 5. A-Priori JWST Cryogenic Spaceflight Qualification (38 K, P_f ≤ 10⁻⁶) *(Lens: `SYSTEMS`)*

Click "3. JWST Cryo-Optics (38 K, P_f ≤ 10⁻⁶)" to simulate an M5 Ti-6Al-4V bolt on a sintered SiC optical bench cooled by ΔT = −255 °C to 38 K under 28 g Ariane 5 launch vibration, then click "🔄 Sync Torque & Preload to 3D .fsm" and inspect the 3D P2 stress field in Results (3) or the 72-DOF Adjoint solver in Coupled (5).

- **Expected Check**: The 1-in-10⁶ Weibull principal-stress bound sets the safe cryogenic torque envelope and syncs the exact clamping preload into the 3D FEA model.

## 3. Theoretical Manual — Chapter 14: Inclined-Plane Sliding-Surface Fastener Mechanics, Junker Vibration Slip, Viscoplastic Boss Creep & Weibull Fracture Qualification

Derives the complete a-priori and empirical mechanics of threaded fixings and snap-fit clips — linking helical sliding-surface tribology (Motosh / Kellermann–Klein + self-tapping thread-forming work) to Junker transverse vibration self-loosening, thick-wall Lamé boss hoop creep, VDI 2230 torsional-tensile bolt yield, Tsai–Wu woven composite peel, and 2-parameter Weibull principal-stress fracture statistics.

### 14.1 Helical Inclined-Plane Sliding-Surface Torque–Preload Relation (Motosh + Thread-Forming)

```text
T_install = F_preload · [ P / (2π)  +  (μ_t · r_t) / cos(α_n)  +  μ_h · r_h ]  +  T_form(d, P, L_eng, σ_y,sub)
K_nut = (T_install − T_form) / (F_preload · d)
```

A screw thread is a fundamental engineering sliding surface: an inclined plane wrapped helically around pitch radius r_t = (d − 0.6495 P)/2 with lead angle β = arctan(P / (2π r_t)) and flank half-angle α_n (30° for ISO 60° metric threads; 15° for 30°-profile Plastite self-tapping screws). The applied screwdriver torque T_install partitions into useful axial clamping work F_preload · P/(2π), helical flank sliding friction, under-head/washer bearing friction, and irreversible plastic/elastic thread-forming torque T_form (or Nyloc prevailing torque T_prev).

### 14.2 Minimal Torque (T_min): Junker Transverse Vibration Slip, Shock G-Load, Creep & Cryo ΔT

```text
F_clamp,min = SF_slip · (m_p · a_vib · g₀) / μ_joint  +  SF_shock · (m_p · a_shock · g₀) / (1 − Φ_k)
ΔF_thermal  = (α_sub − α_screw) · ΔT · L_eng · (k_b · k_m) / (k_b + k_m)
F_install,min = (F_clamp,min + max(0, −ΔF_thermal)) / η_creep(t, T)
```

Under transverse knock or harmonic vibration a_vib, Junker (1969) demonstrated that self-loosening begins the instant transverse shear force F_trans exceeds the clamped interface friction μ_joint · F_clamp, causing radial micro-slip across the thread flanks. To guarantee zero release over a mission duration t (up to a 10-year JWST L2 cryogenic mission), the initial installation preload F_install,min must exceed F_clamp,min after accounting for polymer viscoplastic stress relaxation η_creep(t) = 1 − C_cr ln(1 + t) and differential thermal contraction ΔF_thermal.

### 14.3 Material-Adaptive Maximal Torque (T_max) & Empirical Weibull Spreadsheet Regression

```text
P_f(σ₁) = 1 − exp[ −(V_eff / V₀) · (σ₁ / σ₀)^m ]   ⇒   ln(−ln(1 − P_f)) = m · ln(σ₁) − m · ln(σ₀)
σ_θ,Lamé = p_rad · (r_o² + r_i²) / (r_o² − r_i²),   σ_vM,bolt = √( (F/A_s)² + 3 · (16 T_th / (π d_s³))² ) ≤ 0.9 R_p0.2
```

Maximal torque T_max is governed by the lowest upper bound among five physical failure modes automatically selected by substrate regime: (1) 2-parameter Weibull principal-stress volume fracture for brittle ceramics/optics (SiC) and cold polymers; (2) thick-walled Lamé hoop stress and viscoplastic creep splitting in thermoplastic bosses (PETG, PA12, ABS); (3) cylindrical shear stripping of internal threads at major diameter d; (4) VDI 2230 combined axial tension + thread-torsion von Mises yield of the fastener core; and (5) Tsai–Wu interlaminar tensile/shear peel U_peel = √((σ_zz/S_z)² + (τ_rz/S_s)²) in anisotropic 3K woven CFRP laminates.

### 14.4 Open Public Standards vs. First-Principles Rederivation (FED-STD-H28, NASA-STD-5020B, NIST §8.1.6.2 & NASA-HDBK-7005)

```text
(a) FED-STD-H28 §2B / ISO 898-1:   A_s = (π/4)·(d − 0.938194·P)²
(b) 60° Fundamental Triangle H = (√3/2)·P:   d_s = (d₂ + d₃)/2 = d − (13√3 / 24)·P = d − 0.93819418·P
(a) NASA-STD-5020B / MIL-HDBK-60:  T = K·d·F + T_prev   ⇔   (b) Motosh 3-Surface Wedge Integral
(a) NASA-HDBK-7005 / Sopwith:      F_1 / F_avg = λ·coth(λ),   λ = L_eng / (1.05·d)
```

Whereas commercial standards such as ISO 898-1, ISO 4762, and VDI 2230 are paywalled, their exact governing tables and equations are publicly established in unencumbered US federal and space-agency standards: FED-STD-H28 §2B (metric 60° screw-thread stress areas), NASA-STD-5020B (spaceflight fastener torque–preload and locking criteria), NIST/SEMATECH e-Handbook §8.1.6.2 (Weibull extreme-value fracture regression with Bernard median ranks), and NASA-HDBK-7005 / CARES (2-parameter Weibull brittle ceramics + Sopwith hyperbolic first-thread load concentration). Fieldsmith evaluates both (a) the quick standard lookup rule and (b) the a-priori continuum/statistical rederivation side-by-side so engineers can apply the standard in one click or verify where empirical shortcuts diverge.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Step 1: Auto-Tap Host & Insert M4 Self-Tapping Screw via MCP (`insert_fixing`) (`insert_fixing`)

Calls `insert_fixing` on the live workspace to automatically subtract the pilot bore, spotface, and thread-bite groove on the fixed object, add the Ti-6Al-4V screw head, and compute the [T_min, T_max] torque envelope.

```json
{
  "tool": "insert_fixing",
  "arguments": {
    "kind": "self_tapping_screw",
    "size": "M4",
    "engagement_mm": 8,
    "substrate_material": "petg",
    "fastener_material": "ti64",
    "vibration_g": 14
  }
}
```

### Step 2: Solve 72-DOF Monolithic Fastener PDE & Sparse Adjoint (`solve_coupled_system`) (`solve_coupled_system`)

Solves `jwst_fastener_fixing_envelope_v1` (12 axial stations × 6 fields: Sopwith hyperbolic thread load concentration q_th(z), Lamé hoop stress σ_θ(z), root principal stress σ₁(z), viscoplastic creep, and Weibull hazard density) with exact Sparse Adjoint sensitivities dΦ/dp.

```json
{
  "tool": "solve_coupled_system",
  "arguments": {
    "system_id": "jwst_fastener_fixing_envelope_v1",
    "design_p": [
      1.45,
      4,
      0.7,
      8,
      10,
      1.65,
      12,
      -20
    ],
    "sync_cad": true
  }
}
```

### Step 3: Solve 3D P2 Tetrahedral FEA under Clamping Preload (`solve_study`) (`solve_study`)

Runs the 3D quadratic (P2) finite-element solver on the auto-tapped boss assembly to verify peak von Mises and principal stress under the recommended clamping preload.

```json
{
  "tool": "solve_study",
  "arguments": {
    "include_fine": false
  }
}
```
