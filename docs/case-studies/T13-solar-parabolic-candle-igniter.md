# T13 — Solar-Focused Optical Lens Candle Ignition, Buoyant Flame, Smoke Plume & Real-Time Video

> **Category**: Multiphysics Case Study · **Level**: Flagship Multi-Physics · **Verification**: V56 Verified (144-DOF Monolithic Newton · 40 Transient Frames · 5 Recap Milestones)
> **Repository Path**: `docs/case-studies/T13-solar-parabolic-candle-igniter.md` · **Starter Model**: `t13-solar-candle-ignition.fsm` · **Verification Reports**: `docs/verification/V56.md`

ParaView-Style 3D Volumetric Ray-Marched Radiant Flame, BK7 Solar Caustic Refraction, C₂₅H₅₂ Pyrolysis, Soot/Smoke Plume, Transient Recap & .webm Video Synthesis

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "T13 — Solar-Focused Optical Lens Candle Ignition & Buoyant Flame"
material alsi10mg
custommaterial nbk7_glass name="N-BK7 Optical Crown Glass (n=1.517)" e=82000 nu=0.206 yield=70 density=2.51 wall=0.8 a=0.0000071 source="Schott N-BK7 optical borosilicate crown glass (n_d=1.5168, V_d=64.17)"
custommaterial paraffin_wax name="Paraffin Candle Wax C25H52" e=450 nu=0.38 yield=12 density=0.90 wall=0.8 a=0.00011 source="Paraffin wax C25H52 (T_melt=58 C, L_f=210 kJ/kg, LHV=42 MJ/kg)"
custommaterial cotton_wick name="Braided Cotton Capillary Wick" e=900 nu=0.32 yield=25 density=0.55 wall=0.6 a=0.000025 source="Porous cellulose capillary wick (K_w=4.5e-11 m2, r_p=18 um)"

let lens_r = 32.5
let lens_z = 111.5
let wick_len = 7.5
let i_solar = 1000
let f_lens = 60
let d_lens = 65

cylinder brass_base r=24 h=6 at=(0, 0, 3)
color "#d4a84f" on brass_base

cylinder candle_column r=16 h=34 at=(0, 0, 20)
material paraffin_wax on candle_column
color "#f5efe2" on candle_column
sphere molten_wax_cup r=13 at=(0, 0, 39) subtract

cylinder candle_wick r=2.2 h=18 at=(0, 0, 35)
material cotton_wick on candle_wick
color "#3a2e28" on candle_wick

box mast_bracket size=(8, 44, 6) at=(0, -21, 3) round=1.0
color "#6d7888" on mast_bracket

box support_mast size=(6, 8, lens_z) at=(0, -42, 0.5 * lens_z) round=1.5
color "#6d7888" on support_mast

box lens_arm size=(6, 42, 5) at=(0, -21, lens_z) round=1.0
color "#6d7888" on lens_arm

cylinder lens_gimbal_outer r=37.0 h=5 at=(0, 0, lens_z)
color "#7b889a" on lens_gimbal_outer
cylinder lens_gimbal_bore r=lens_r h=8 at=(0, 0, lens_z) subtract

cylinder optical_glass_lens r=lens_r h=6 at=(0, 0, lens_z)
material nbk7_glass on optical_glass_lens
color "#88c8ff" on optical_glass_lens

material cucrzr on brass_base

region base_pad = brass_base.bottom
region wick_hot_zone = candle_wick.side
region candle_wall = candle_column.side
region lens_top_face = optical_glass_lens.top

support clamp_base base_pad fixed=xyz
load lens_gravity lens_top_face force=(0, 0, -12) case="1. Solar Caustic Ignition & Thermal Plume"
temperature wick_ignition wick_hot_zone temp=397
convection ambient_plume candle_wall h=18 tinf=22
heatflux solar_lens_heating lens_top_face q=0.015

solve tref=22
```

## 2. Step-by-Step Guided Walkthrough

### 1. ParaView-Style 3D Volumetric Radiant Flame, Solar Caustic Cone & Smoke Plume in the Main Viewport *(Lens: `SHAPE`)*

Loading T13 places the 9-body 3D Solar-Focused Candle Assembly (brass base, paraffin column, concave molten wax cup, braided cotton wick, and overhead Ø 65 mm biconvex BK7 optical glass lens) in the main 3D viewport alongside the live "3D Volumetric Solar Candle Flame & Plume" HUD (`[data-testid="coupled-viewport-hud"]`). Switch the Volumetric Transfer Function tabs (`Radiant Flame`, `Temp T_gas`, `Soot / Smoke f_v`, `Blue OH*`, `Updraft u_z`) to inspect the blue OH*/CH* chemiluminescence base sheet, the incandescent Planckian soot core (`~2100 K`), the Beer–Lambert absorbing grey smoke plume aloft, the golden converging solar caustic ray cone (`C ≈ 2355×`), and the buoyant convective entrainment streamlines.

- **Expected Check**: The 3D viewport renders the volumetric radiant flame, blue OH* base, smoke plume, solar caustic cone, and buoyant airflow directly over the 3D candle and lens.

### 2. Scrubbing Time-Dependent Results, Milestone Recap Cards & Real-Time .webm Video Generation *(Lens: `SHAPE`)*

Directly in the bottom-left 3D Viewport HUD (`[data-testid="coupled-viewport-hud"]`), use the Transient Timeline Scrubber (`[data-testid="transient-time-slider"]`, 40 frames from t = 0.00 s to 5.50 s) and click the 5 Milestone Recap Cards (`[data-testid="recap-event-0"]` .. `4`: 1. Solar Caustic Focus → 2. Wax Pool Melting at 331 K → 3. Pyrolysis Vapor Plume at 620 K → 4. Flash Auto-Ignition at 670 K → 5. Steady Buoyant Diffusion Flame & Smoke Plume). Then click "🎬 Render Video" (`[data-testid="render-transient-video"]`) to record a real-time 30 fps WebM video with embedded scientific telemetry HUD.

- **Expected Check**: Clicking any milestone recap card jumps the 3D volumetric scene to that physical regime, and "Render Video" produces an inline playable `.webm` video (`[data-testid="transient-video-player"]`) + download link.

### 3. ParaView Colourmap Transfer Functions & 3D Volumetric Iso-Shells in Results (3) *(Lens: `RESULTS`)*

Tap "Go to Results →" (tab 3) to solve the 3D thermo-mechanical FEA study on the candle, wick, and optical lens assembly. Switch to "Temperature" or "Stress", select the new ParaView "Flame" (Blackbody) or "Turbo" colourmap (`[data-testid="results-colormap"]`), and toggle "3D Volume" (`[data-testid="results-volume-toggle"]`) to render 5 nested additive volumetric iso-shells highlighting the thermal plume and stress gradients inside the 3D mesh.

- **Expected Check**: In Results (3), the Flame/Turbo colourmaps and 3D Volume iso-shells render translucent glowing scalar contours across the 3D FEA mesh.

### 4. Full 144-DOF Adjoint Sensitivity Matrix & Epistemic UQ in Coupled Systems *(Lens: `SYSTEMS`)*

Tap "Go to Coupled Systems →" to open the full 3-column Coupled Systems Workbench directly on `solar_candle_ignition_v1`. Inspect the p10 / p50 / p90 epistemic uncertainty bands, the axial flame & smoke attribution profiles, and the sparse Adjoint sensitivity matrix dΦ/dp, or click "Step 1 Adjoint Iteration (−dΦ/dp)" to optimize lens focus and suppress smoke.

- **Expected Check**: The Coupled Systems Workbench opens directly on the Solar-Focused Optical Lens Candle Ignition & Buoyant Flame pack (`solar_candle_ignition_v1`).

## 3. Theoretical Manual — Chapter 13 — Solar Caustic Optics, Paraffin Diffusion Flame Chemistry, Buoyant Plume & Volumetric Rendering (V56)

Coupled thick-lens solar refraction, C₂₅H₅₂ phase change and capillary pyrolysis, Burke–Schumann mixture-fraction combustion, OH* chemiluminescence, Moss–Brookes soot/smoke kinetics, and ParaView emission-absorption volume rendering.

### 13.1 Thick-Lens BK7 Solar Caustic & Wick Auto-Ignition Delay

```text
1 / f = (n_BK7 − 1) · [ 2 / R_lens − (n_BK7 − 1) · t_c / (n_BK7 · R_lens²) ]
w(Δz) = √( (f · θ_sun)² + σ_sph² + (D_lens · |z_lens − f| / (2 f))² ),   C_opt = τ_opt · (D_lens / (2 w(Δz)))²
```

Sunlight subtends a half-angle θ_sun = 4.65 mrad (0.266°). Refracted through a biconvex BK7 borosilicate crown glass lens (n = 1.5168), the beam converges to a caustic waist w(0) limited by the solar disk image and spherical aberration σ_sph, achieving optical concentrations C_opt > 2000× and peak focal fluxes > 4000 kW/m². Axial defocus Δz = z_lens − f expands the spot radius conically, reducing absorbed power on the braided cotton wick and lengthening the time to cross T_melt = 331 K, T_vap = 620 K, and T_ign = 670 K.

### 13.2 Burke–Schumann Mixture Fraction, Blue OH* Base & Soot/Smoke Plume

```text
C₂₅H₅₂ + 38 O₂ → 25 CO₂ + 26 H₂O   (Z_st = 0.0637 in air)
u_z(z) = √( u_0² + 2 g β ∫ (T_cl(ζ) − T_∞) dζ ),   d(r_f² u_z)/dz = 2 α_ent r_f u_z
```

Once ignited, radiant feedback melts a concave liquid paraffin pool that feeds the wick by Darcy–Lucas–Washburn capillary suction. Vaporized C₂₅H₅₂ mixes with radially entrained air (α_ent ≈ 0.115); along the stoichiometric reaction sheet Z(r, z) = Z_st, excited OH*/CH* radicals emit royal-blue chemiluminescence (431 nm / 308 nm) near the oxygen-rich base. Higher in the fuel-rich interior (0.08 < Z < 0.25, T > 1300 K), polycyclic aromatic hydrocarbons nucleate carbonaceous soot (f_v ~ 1–4 ppm) whose Planckian T⁴ incandescence produces the warm yellow-white candle glow. Unoxidized soot escaping the tip cools into the dark Beer–Lambert absorbing smoke plume.

### 13.3 Thermophoretic Soot Deposition on the Overhead Glass Lens & Optical Fouling

```text
v_th = −K_th · (ν_gas / T_gas) · ∇T   (K_th ≈ 0.54)
δ_soot(r, t) = δ_soot,0(t) · exp(−(r / r_smudge)²),   τ_lens(t) = τ_clean · exp(−α_ext · δ_soot,0(t))
```

Because the optical concentrator lens sits directly above the candle to focus overhead sunlight, the rising buoyant smoke plume impinges on the cooler bottom face of the BK7 glass lens and deflects radially outward. Across the steep thermal boundary layer between the hot plume and cooler glass, Talbot–Brock thermophoresis drives sub-micron carbonaceous soot aggregates onto the lens surface, building a radially graded velvety black core and amber-brown PAH tar halo (δ_soot ~ 25–150 nm) that progressively attenuates lens transmittance τ_lens(t).

## 4. Reproducible Automation (MCP / REST API & CLI)

### 1. Solve Defocused Solar Candle Scenario (Slow Ignition & Heavy Smoke) via MCP (`solve_coupled_system`)

Evaluate Scenario B (D_lens = 55 mm, z_lens = 92 mm defocused, L_wick = 12 mm long wick) and sync its 3D .fsm CAD twin.

```json
{
  "tool": "solve_coupled_system",
  "arguments": {
    "system_id": "solar_candle_ignition_v1",
    "design_p": [
      55,
      68,
      92,
      780,
      12,
      0.21
    ],
    "sync_cad": true
  }
}
```

### 2. Run 3 Sparse Adjoint (−dΦ/dp) Optimisation Steps to Refocus Lens & Suppress Smoke (`optimise_coupled_system`)

Use exact sparse Adjoint gradients dΦ/dp across all 144 DOFs to refocus the optical lens, enlarge collection aperture, and trim wick smoke.

```json
{
  "tool": "optimise_coupled_system",
  "arguments": {
    "system_id": "solar_candle_ignition_v1",
    "steps": 3,
    "sync_cad": true
  }
}
```
