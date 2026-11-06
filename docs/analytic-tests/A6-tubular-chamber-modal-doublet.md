# A6 — Analytic Benchmark A6: Axisymmetric Tubular Thrust Chamber Cantilever Modal Bending Doublet (V13 / V60d)

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Verified by V13 & V60(d) · Cowper–Timoshenko Tubular Eigenmodes · PASS (1.53%)
> **Repository Path**: `docs/analytic-tests/A6-tubular-chamber-modal-doublet.md` · **Starter Model**: `a6-tubular-chamber-modal.fsm` · **Verification Reports**: `docs/verification/V13.md`, `docs/verification/V60.md`

Verify Fieldsmith’s P2 consistent-mass modal eigensolver on a clamped Ti-6Al-4V tubular thrust chamber (r_i = 6 mm, r_o = 12 mm, L = 160 mm) against closed-form hollow-tube Euler–Bernoulli and Cowper–Timoshenko beam theory.

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| Hollow Tube Cross-Section Properties (r_i = 6 mm, r_o = 12 mm) | `A = π(r_o² − r_i²) = 339.292 mm²,  I = (π/4)(r_o⁴ − r_i⁴) = 15,268.14 mm⁴` | **r_gyr = √(I/A) = 6.7082 mm** | **Exact Section** | 0.00% | Exact |
| Fundamental Cantilever Bending Doublet Frequency (Modes 1 & 2) | `f_1,EB = (1.8751)²/(2π L²) √(EI/ρA) = 743.85 Hz;  f_1,Timo (κ_tube = 0.603) = 728.26 Hz` | **728.26 Hz** | **717.12 Hz / 717.12 Hz** | 1.53% | ≤ 3.0% |
| Axisymmetric Orthogonal Doublet Split |f₂ − f₁| / f̄ | `SO(2) rotational symmetry about Z axis ⇒ f₁ (X-bending) = f₂ (Y-bending)` | **0.000% (Degenerate Pair)** | **0.000% (717.12 vs 717.12 Hz)** | 0.000% | ≤ 1.5% |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "A6 — Axisymmetric Tubular Thrust Chamber Modal Benchmark (V60d)"
material ti64

cylinder outer r=12 h=160 at=(0, 0, 80)
cylinder bore r=6 h=160 at=(0, 0, 80) subtract
region flange = outer.bottom
support clamp flange fixed=xyz
load nudge outer.top force=(10, 0, 0)
solve
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect the Clamped Ti-6Al-4V Tubular Thrust Chamber Shell *(Lens: `SETUP`)*

Open Setup (2). A hollow circular Ti-6Al-4V thrust chamber tube (inner radius r_i = 6 mm, outer radius r_o = 12 mm, length L = 160 mm, E = 114,000 MPa, ν = 0.34, ρ = 4.43 g/cm³ = 4.43×10⁻⁹ N·s²/mm⁴) is clamped at its injector flange z = 0 (fixed=xyz).

- **Expected Check**: The bottom annulus z = 0 is clamped while the upper 160 mm tubular shell is free to vibrate laterally in X and Y.

### 2. Verify the Degenerate Fundamental Bending Doublet (717.1 Hz) in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3) and inspect the Modal frequencies. For a hollow circular tube with m = r_i/r_o = 0.5, Cowper’s (1966) exact elasticity shear coefficient is κ = 6(1+m²)²(1+ν)/[(7+6ν)(1+m²)² + (20+12ν)m²] = 0.6032, shifting the fundamental cantilever frequency from f_1,EB = 743.85 Hz to f_1,Timo = 728.26 Hz. Fieldsmith’s P2 consistent-mass eigensolver finds the orthogonal X/Y bending pair at 717.12 Hz and 717.12 Hz (1.53% from Timoshenko, 0.000% doublet split).

- **Expected Check**: Modes 1 and 2 form an orthogonal transverse bending doublet at ~717 Hz.

## 3. Theoretical Manual — Analytic Verification A6 — Cowper–Timoshenko Hollow-Tube Eigenmodes & SO(2) Doublet Degeneracy (V13 / V60d)

Derives the generalised eigenvalue problem K φ = ω² M φ on P2 tetrahedra and compares the fundamental bending doublet of a hollow circular tube against Euler–Bernoulli and Cowper–Timoshenko beam theory.

### A6.1 Cowper (1966) Shear Coefficient for a Hollow Circular Tube

```text
A = π(r_o² − r_i²),     I = (π/4)(r_o⁴ − r_i⁴),     m = r_i / r_o
κ_tube = 6(1 + m²)²(1 + ν) / [(7 + 6ν)(1 + m²)² + (20 + 12ν)m²]
f_1,Timo = f_1,EB / √(1 + 4.6478 [I/(A L²) + E I / (κ_tube G A L²)])
```

Because an axisymmetric tube has identical second moments of area I_xx = I_yy = (π/4)(r_o⁴ − r_i⁴) about every transverse axis through the origin, its transverse bending modes occur in degenerate orthogonal pairs (ω_{2k−1} = ω_{2k}).

## 4. Reproducible Automation (MCP / REST API & CLI)

### Compute First 4 Natural Modes via API (`solve`)

Run the modal eigensolver on the Ti-6Al-4V tubular thrust chamber.

```json
{
  "tool": "solve",
  "arguments": {
    "kind": "modes",
    "modes": 4,
    "dofs": 30000
  }
}
```
