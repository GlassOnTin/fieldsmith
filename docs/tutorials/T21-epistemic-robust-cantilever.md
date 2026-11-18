# T21 — Epistemic Uncertainty Quantification (O(1)-Solve 3D Adjoint UQ) & Robust p90/p10 Sizing

> **Category**: Guided Tutorial · **Level**: Advanced · **Verification**: Verified by V63 · O(1)-Solve 3D FOSM Adjoint UQ + Robust Quantile SQP
> **Repository Path**: `docs/tutorials/T21-epistemic-robust-cantilever.md` · **Starter Model**: `t21-epistemic-robust-cantilever.fsm` · **Verification Reports**: `docs/verification/V63.md`

Declare machining tolerances, load scatter, and material yield variability directly in `.fsm` (`uncertainty ...`), propagate p10 / p50 / p90 quantiles and ranked variance shares in O(1) linear FEA solves, and size a 3D SS316L cantilever against robust `p90.max_vm_elem` and `p10.min_safety_factor` contracts.

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
name "Epistemic Robust SS316L Cantilever"
material ss316l

let d = 12

box beam size=(60, 12, d) at=(30, 0, 0)

region root = beam.nx
region tip = beam.px

support clamp root fixed=xyz
load push tip force=(0, 0, -550)

solve build=(0, 0)

uncertainty "Machining depth tolerance" param="d" std=0.3 unit="mm"
uncertainty "Tip load scatter" param="push.force.2" cv=0.08 unit="N"
uncertainty "SS316L yield scatter" param="ss316l.yield" cv=0.06 unit="MPa"

assert "Robust 90th-percentile peak von Mises stress bounded" metric="p90.max_vm_elem" target="<= 175.0" tol="<= 175.0" unit="MPa"
assert "Robust 10th-percentile yield safety factor bounded" metric="p10.min_safety_factor" target=">= 1.15" tol=">= 1.15" unit="-"
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect Epistemic Quantiles & Variance Attribution in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3) and click **Compute UQ (3)**. The discrete 3D boundary-velocity adjoint propagates `d` (±0.30 mm), `push.force.2` (CV 8%), and `ss316l.yield` (CV 6%) in 4 linear FEA solves total (1 primal + 3 adjoints for σ_vM, ‖u‖, and layer peel), reporting `p10`, `p50`, `p90`, `±σ`, and the dominant variance driver per metric.

- **Expected Check**: `results-uq-table` reports `p90 > p50 > p10` on `max_vm_elem` and `min_safety_factor` with `solves_run == 4`.

### 2. Run Robust Contract-Constrained Sizing *(Lens: `RESULTS`)*

Select **Min Mass (g)** and click **Optimise**. The SQP optimiser differentiates `p90.max_vm_elem` and `p10.min_safety_factor` analytically without extra inner UQ FEA solves, trimming `d` down to the active robust reliability boundary while retaining epistemic safety margin above deterministic `p50` sizing.

- **Expected Check**: Both `p90.max_vm_elem <= 175 MPa` and `p10.min_safety_factor >= 1.15` pass on the optimised design.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Propagate 3 epistemic uncertainties in O(1) FEA solves

```json
{
  "tool": "uq",
  "arguments": {
    "dofs": 4000
  }
}
```

### Verify initial robust p90/p10 contracts

```json
{
  "tool": "verify",
  "arguments": {
    "dofs": 4000
  }
}
```

### Add Young's modulus uncertainty

```json
{
  "tool": "upsert_uncertainty",
  "arguments": {
    "label": "Modulus scatter",
    "param": "ss316l.e",
    "cv": 0.05,
    "unit": "MPa"
  }
}
```

### Re-evaluate 4-input epistemic UQ in O(1) solves

```json
{
  "tool": "uq",
  "arguments": {
    "dofs": 4000
  }
}
```

### Remove temporary modulus uncertainty

```json
{
  "tool": "delete_uncertainty",
  "arguments": {
    "index": 3
  }
}
```

### Optimise beam depth d for minimum mass under robust p90/p10 contracts

```json
{
  "tool": "optimise",
  "arguments": {
    "objective": "mass_g",
    "params": [
      "d"
    ],
    "bounds": {
      "d": [
        8.5,
        14
      ]
    },
    "max_iters": 6,
    "dofs": 4000,
    "apply": true
  }
}
```

### Re-verify robust p90/p10 contracts on mass-trimmed beam

```json
{
  "tool": "verify",
  "arguments": {
    "dofs": 4000
  }
}
```

---
*Generated from `examples/tutorials/T21-epistemic-robust-cantilever.fsm` by `node scripts/gen-sheet-docs.mjs`.*
