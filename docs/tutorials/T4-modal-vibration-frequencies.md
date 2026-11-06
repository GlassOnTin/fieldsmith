# T4 — Natural Vibration Frequencies: Why Parts Hum, Shake or Resonate

> **Category**: Guided Tutorial · **Level**: Intermediate · **Verification**: Lesson 4 · Intermediate · Vibration & Resonance
> **Repository Path**: `docs/tutorials/T4-modal-vibration-frequencies.md` · **Starter Model**: `t4-modal-tuning-fork.fsm` · **Verification Reports**: `docs/verification/V13.md`

Every structure has natural frequencies (measured in Hertz, cycles per second) where it wants to vibrate like a tuning fork. Find and animate the first 4 vibration modes.

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "T4 — Optical Sensor Mount (Modal)"
material alsi10mg

box flange size=(36, 36, 6) at=(0, 0, 3)
cylinder stem r=6 h=34 at=(0, 0, 20) blend=3
box stage size=(28, 28, 8) at=(0, 0, 38) blend=2

region base = flange.nz
region top = stage.pz
support fix_base base fixed=xyz
load gravity_tip top force=(0, 200, -300)
solve modes=4
```

## 2. Step-by-Step Guided Walkthrough

### 1. What Are Natural Frequencies and Mode Shapes? *(Lens: `SHAPE`)*

If you pluck a ruler held over the edge of a desk, it vibrates back and forth at a specific pitch — its fundamental natural frequency (measured in Hz, or cycles per second). If a motor, fan, or vehicle engine shakes a bracket at that exact frequency, vibrations amplify dramatically (resonance). Here we have a 3D-printed aluminium (AlSi10Mg) optical sensor pedestal. Open the Inspector with nothing selected to see "Modes = On (Count = 4)".

- **Expected Check**: The pedestal consists of a bolted base flange, a slender cylindrical stem, and a heavy top sensor stage — a classic mass-on-a-spring geometry.

### 2. Animate & Record Real-Time .webm Video of the 4 Vibration Modes in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3). In the "Modes" row of the Results card, tap Mode 1, 2, 3, or 4 and tap "Play". Mode 1 and Mode 2 are side-to-side rocking of the heavy top stage in X and Y; Mode 3 is twisting (torsion) around the vertical stem; Mode 4 is vertical bouncing. Click the 4 Phase Milestone Recap pills (`1. +Peak (0°)`, `2. Node (90°)`, `3. −Peak (180°)`, `4. Full Cycle (360°)`) or click "🎬 Render Video" (`[data-testid="results-render-video"]`) to synthesise a real-time 30 fps `.webm` video of the vibrating mode shape!

- **Expected Check**: All 4 natural frequencies (in Hz) are listed; tapping Play animates the 3D flexing motion and "🎬 Render Video" records a full harmonic oscillation cycle to `.webm`.

## 3. Theoretical Manual — Chapter 4 — Mass Matrices, Stiffness Matrices & Eigenvalue Vibrations

For a single weight m on a spring of stiffness k, the natural frequency is ω = √(k / m). For a 3D solid, k becomes the stiffness matrix K and m becomes the mass matrix M.

### 4.1 The Generalised Eigenvalue Problem (K φ = ω² M φ)

```text
M ü(t) + K u(t) = 0   ⇒   K φ_k = ω_k² M φ_k,   f_k = ω_k / (2π)  [Hz]
```

When a structure vibrates harmonically without external damping, Newton’s second law (Force = Mass × Acceleration) balances elastic restoring forces (−K u) against inertial forces (M ü). Substituting a sinusoidal motion u(t) = φ cos(ω t) turns the differential equation into an algebraic eigenvalue problem: each eigenvector φ_k is a 3D mode shape, and each eigenvalue ω_k² gives the natural frequency f_k in Hz.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Compute the First 4 Natural Frequencies via API (`solve`)

Ask the solver for the lowest 4 natural vibration frequencies (in Hz) of the sensor pedestal.

```json
{
  "tool": "solve",
  "arguments": {
    "kind": "modes",
    "n": 4,
    "dofs": 30000
  }
}
```
