# T3 — How Heat Warps Parts: Thermal Conduction & Expansion Stress

> **Category**: Guided Tutorial · **Level**: Intermediate · **Verification**: Lesson 3 · Intermediate · Heat & Thermal Stress
> **Repository Path**: `docs/tutorials/T3-thermo-elastic-bracket.md` · **Starter Model**: `t3-thermo-elastic-bracket.fsm` · **Verification Reports**: `docs/verification/V19.md`, `docs/verification/V20.md`

See how heat flows from a 220 °C hot pin to a 20 °C cold mount, and how restrained thermal expansion creates internal stress even with zero external force.

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "T3 — Thermo-Elastic Manifold Bracket"
material ss316l

box base size=(60, 40, 8) at=(30, 20, 4)
box upright size=(8, 40, 40) at=(4, 20, 20) blend=8
cylinder hole1 r=3.25 h=20 at=(20, 10, 4) subtract
cylinder hole2 r=3.25 h=20 at=(20, 30, 4) subtract
cylinder hole3 r=4.25 h=20 at=(4, 20, 28) rotate=(0, 90, 0) subtract

region mount = hole1.side + hole2.side
region load_pin = hole3.side
support s1 mount fixed=xyz
load l1 load_pin force=(103.5, 0, -386.4) case="Mechanical"
temperature t_cold mount temp=20 case="Thermal"
temperature t_hot load_pin temp=220 case="Thermal"
solve tref=20
```

## 2. Step-by-Step Guided Walkthrough

### 1. Why Heating a Bolted Part Creates Stress *(Lens: `SETUP`)*

Almost all metals and plastics expand slightly when heated. If a part floats freely in space and warms up uniformly, it grows slightly larger with zero internal stress. But if the part is bolted down to a cold frame ("mount" held at 20 °C) while its upper port touches a hot exhaust pipe ("load_pin" at 220 °C), the hot metal tries to expand against the cold bolts. Switch to Setup (2) to inspect the two separate load cases on this 316L stainless-steel bracket: "Mechanical" (400 N force) and "Thermal" (20 °C to 220 °C temperature difference).

- **Expected Check**: Setup tree shows Support s1 (fixed bolts), Load l1 (Mechanical case), and Temperatures t_cold (20 °C) and t_hot (220 °C).

### 2. View the Temperature Map, ParaView Flame Volume & Real-Time Thermal Video *(Lens: `RESULTS`)*

Switch to Results (3). First tap "Temperature" in the results mode bar, set Colourmap (`[data-testid="results-colormap"]`) to "Flame", and toggle "3D Volume" (`[data-testid="results-volume-toggle"]`) to see how heat conducts volumetrically from the 220 °C upper hole down to the 20 °C mounting bolts. Use the Transient Ramp slider or click the 4 Milestone Recap pills (`0% → 35% → 70% → 100%`) and click "🎬 Render Video" (`[data-testid="results-render-video"]`) to synthesise a real-time `.webm` video of the thermal conduction and expansion stress!

- **Expected Check**: The Temperature view shows a smooth 20 °C → 220 °C volumetric gradient; the Thermal stress view shows high stress around the two base bolt holes where expansion is prevented, and "🎬 Render Video" exports a 30 fps `.webm` video.

## 3. Theoretical Manual — Chapter 3 — Steady-State Heat Conduction & Thermo-Elastic Strain

Fieldsmith solves thermal stress in two linked steps on the same mesh: first it finds the steady-state temperature T(x) everywhere inside the part, and then it converts local temperature rise into thermal expansion strain.

### 3.1 Fourier’s Law of Heat Conduction & Boundary Conditions

```text
−∇·(k ∇T) = 0  in Ω
Fixed Temp (Dirichlet): T = T_0,   Convection (Robin): −k ∂T/∂n = h·(T − T_air)
```

Heat flows from hot to cold at a rate proportional to the material’s thermal conductivity k (Fourier’s law). At steady state, heat entering any tiny volume equals heat leaving it, giving Laplace’s equation −∇·(k ∇T) = 0. Boundaries can specify a known surface temperature (called a Dirichlet condition), a known heat-input rate in Watts/mm² (Neumann condition), or cooling by surrounding air/liquid with heat-transfer coefficient h (Robin convection condition).

### 3.2 Thermal Expansion Strain (ε_th = α · ΔT)

```text
ε_th(x) = α · (T(x) − T_ref) · [1, 1, 1, 0, 0, 0]ᵀ
σ = D · (ε_total − ε_th)
```

Every material has a coefficient of thermal expansion α (how much a 1 mm bar expands per 1 °C temperature rise above the reference temperature T_ref). Notice that mechanical stress σ depends only on the elastic strain (ε_total − ε_th): if a bar is free to expand so ε_total = ε_th, stress σ is zero; if fixed bolts force ε_total = 0, a compressive thermal stress σ = −D ε_th builds up.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Increase Hot Pin Temperature to 320 °C via API (`set_let`)

Raise the hot boundary temperature from 220 °C to 320 °C and observe how thermal stress scales with ΔT.

```json
{
  "tool": "set_let",
  "arguments": {
    "name": "t_hot.celsius",
    "value": 320
  }
}
```

### Solve Both Mechanical & Thermal Load Cases via API (`solve`)

Run the coupled heat-conduction and thermo-elasticity solver and return the per-case stress summary.

```json
{
  "tool": "solve",
  "arguments": {
    "kind": "solve",
    "dofs": 30000
  }
}
```
