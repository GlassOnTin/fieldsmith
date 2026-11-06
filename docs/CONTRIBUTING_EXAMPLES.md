# Contributing Public Examples, Guided Tutorials & Analytic Verification Benchmarks

Fieldsmith uses a **single-file Literate `.fsm` architecture** inspired by Stéphane Popinet's [**Gerris**](https://gfs.sourceforge.net/tests/tests/index.html) and [**Basilisk**](http://basilisk.fr/src/test/README): **every analytic verification test, guided tutorial, and hardware case study is completely defined by a single standalone `.fsm` file under `examples/`**.

Because `#` is a standard comment character in the `.fsm` grammar (`crates/fieldsmith-doc/src/fsm.rs`), each `.fsm` file carries both:
1. A structured `# === FIELDSMITH LITERATE CASE ===` header defining its metadata, closed-form analytic reference checks (`# @check:`), interactive guided steps (`# @step:` / `# @expected:`), engineering theory sections (`# @theory_section:` / `# @theory_eq:` / `# @theory_body:`), and MCP automation steps (`# @mcp:`).
2. The executable `fieldsmith 1` parametric CAD + multiphysics model below `# === END LITERATE HEADER ===`, which parses directly in the Rust CLI (`fieldsmith solve`), verification suite (`cargo test -p fieldsmith-doc`), and Web/Android app.

| Track | Source `.fsm` Directory | Generated Markdown Mirror | ID Prefix | Purpose |
| :--- | :--- | :--- | :--- | :--- |
| **📐 Analytic Verification Test** | `examples/analytic/` | `docs/analytic-tests/` + `docs/verification/` | `A1`–`A14`, `A15`… | Closed-form mathematical solution compared quantitatively against Fieldsmith P2 FEA with automated CI tolerance gates. |
| **📘 Guided Tutorial** | `examples/tutorials/` | `docs/tutorials/` | `T1`–`T7`, `T14`–`T15` | Step-by-step instructional lesson teaching a modelling, meshing, lattice, or physics workflow. |
| **🚀 Multiphysics Case Study** | `examples/case-studies/` | `docs/case-studies/` | `T8`–`T13`, `T16`–`T17` | End-to-end analysis of real open-source hardware (e.g. rocket chambers, CVD coaters, SOFC stacks, RF structures). |
| **📦 Starter `.fsm` Template** | `examples/` | — | `<slug>.fsm` | Compact starter `.fsm` script in the `Examples ▾` toolbar menu. |

---

## Option 1: Export a Single-File Literate `.fsm` Directly from Fieldsmith (Fastest)

1. Build or load your model in the Fieldsmith workspace and click **Solve** so live FEA metrics are available.
2. Open **Tutorials & Analytic Tests (`?`)** and click **`➕ Submit Case`** in the top bar (or open tab **`5. ➕ Submit / Export Case`**).
3. Choose your category (`Analytic Verification Test`, `Guided Tutorial`, or `Multiphysics Case Study`), enter the title, governing equation, and closed-form reference value.
4. Click **`⬇ Download Literate <ID>-<slug>.fsm`** and drop the file into:
   - `examples/analytic/` (for `category: analytic`)
   - `examples/tutorials/` (for `category: tutorial`)
   - `examples/case-studies/` (for `category: case-study`)
5. **No TypeScript code edits required**: [`web/src/doc/literate-fsm.ts`](../web/src/doc/literate-fsm.ts) auto-discovers all `.fsm` files in `examples/{analytic,tutorials,case-studies}/*.fsm` at build time via `import.meta.glob`, and `cargo test -p fieldsmith-doc all_examples_and_literate_fsm_files_parse` validates the `.fsm` syntax.

---

## Option 2: Authoring a Literate `.fsm` File by Hand

Create a new `.fsm` file in `examples/analytic/`, `examples/tutorials/`, or `examples/case-studies/`:

```fsm
fieldsmith 1
# === FIELDSMITH LITERATE CASE ===
# @id: A15
# @slug: saint-venant-shaft
# @number: 32
# @category: analytic
# @title: Saint-Venant Circular Shaft Shear & Bending Benchmark
# @subtitle: Solid circular shaft under tip transverse shear (V61)
# @level: Intermediate
# @duration: 5 min
# @tags: Analytic Verification, Linear Elasticity, Timoshenko Beam, V61
# @summary:
#   Verifies tip deflection of a circular Ti-6Al-4V shaft against Timoshenko beam theory.
# @check: Tip deflection δ_y || δ = P L³ / (3 E I) + P L / (κ G A) || 0.0452 || 0.0454 || mm || +0.44% || 2.0% || V61
# ---
# @step: Solve P2 Tetrahedral Mesh || Check || [data-testid="solve-btn"]
#   Click Solve to run the P2 tetrahedral solver and compare against Timoshenko theory.
# @expected:
#   Tip deflection matches the closed-form prediction within 2%.
# ---
# @theory_title: Saint-Venant & Timoshenko Circular Shaft Theory
# @theory_section: Governing Equation
# @theory_eq:
#   I = π r⁴ / 4,   κ_circle = 6 (1 + ν) / (7 + 6 ν)
# @theory_body:
#   For a solid circular cross-section, Cowper's shear correction factor κ_circle accounts for parabolic transverse shear warpage across the circular face.
# === END LITERATE HEADER ===

name "A15 — Saint-Venant Circular Shaft Benchmark"
material ti64

cylinder shaft r=10 h=80 at=(0, 0, 40)
region base = shaft.bottom
region top = shaft.top
support clamp base fixed=xyz
load tip_shear top force=(0, 500, 0)
solve
```

Run `cargo test -p fieldsmith-doc all_examples_and_literate_fsm_files_parse` to verify that the `.fsm` file parses and validates cleanly.
