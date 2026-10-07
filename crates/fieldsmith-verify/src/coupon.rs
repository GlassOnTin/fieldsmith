//! Resolved-FE coupon measurement of TPMS homogenised stiffness: mesh an N³ block of unit
//! cells with isosurface stuffing, load it in uniaxial compression with free-slip platens, and
//! read the apparent (homogenised) modulus `E*` from the volume-weighted strain over an interior
//! band. This is the reference measurement behind the engine's stiffness laws
//! (`fieldsmith-core/src/fill.rs`, fitted from these numbers) and V6's check.
//!
//! Coupon conventions (measured, see `examples/tpms-coupon.rs` for the full rationale):
//! * Sheet: 3 cells, 1-cell strain band, h = t (the wall thickness); network: 4 cells, band 1,
//!   h = 0.5·2R (struts need two mesh cells across).
//! * `--shift s` offsets the lattice pattern inside the coupon to defuse the tie-alignment
//!   lottery that can leave the load face without planar patches.
//! * Loading: traction −σ·nₓ on the material patches of the x = L face (exact-planar patches
//!   only; tilted transition triangles at the wall-boundary curve would apply 1/nₓ too much
//!   axial force), ux = 0 on the x = 0 patches, y and z free (free-slip platens). Two point
//!   constraints (uy = uz at the origin corner; uy on the y = 0 face near the far x edge; both
//!   ≥ 1 cell from the strain band) remove the rigid modes the platens leave.
//! * Measurement: E* = Σ / ε with Σ the macroscopic section stress (total applied force over
//!   the L² cross-section) and ε the volume-weighted least-squares slope of ux against x over
//!   the interior band. Point probes land in void at low density; the volume weighting uses the
//!   mesh itself so it measures the material that is actually present.
//! * Isolated fragments: at some (family, mode, ρ) combinations the level set pinches small
//!   blobs off the main labyrinth. A floating blob cannot carry load, so only the largest
//!   connected component (face adjacency of the corner-vertex tets) is kept and the dropped
//!   tets are reported.

use fenics_element::{FiniteElement, P2Tetrahedron};
use fieldsmith_doc::{Family, FillMode};
use fieldsmith_field::lattice::{calibration, lattice_field};
use fieldsmith_mesh::{quality, stuff, TetMesh, V3};
use fieldsmith_solve::p2::face_nodes;
use fieldsmith_solve::stress::element_stress;
use fieldsmith_solve::{solve, Material, P2Mesh, Problem, SolverOptions};
use std::time::Instant;

/// Exact signed distance of the axis-aligned box [0, L]³ (negative inside).
pub fn box_sdf(p: V3, l: f64) -> f64 {
    let q = [
        (-p[0]).max(p[0] - l),
        (-p[1]).max(p[1] - l),
        (-p[2]).max(p[2] - l),
    ];
    let o = q.map(|c| c.max(0.0));
    let outside = (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt();
    let inside = q[0].max(q[1]).max(q[2]);
    outside + inside.min(0.0)
}

/// Thinnest strut radius R of a network lattice: sample one periodic unit cell on a 160³ grid,
/// take the material region {lattice ≤ 0}, compute its distance-to-void field with a two-pass
/// 3D chamfer transform, and read R as the minimum over the field's local maxima (strut centre
/// lines; a chamfer distance is a radius-like quantity, so its per-strut maximum sits on the
/// centre line and its minimum over struts is the thinnest strut). Voxels within one grid step
/// of the sample boundary are excluded (out-of-bounds counts as void), so struts cut by the
/// cell face do not read short — another cell of the periodic pattern holds their centre.
/// Returns R in mm.
pub fn strut_min_radius(family: Family, cell: f64, param: f64) -> f64 {
    let n = 160usize;
    let s = cell / n as f64;
    let nn = n * n * n;
    let mut d = vec![1.0e6f32; nn];
    for i in 0..n {
        for j in 0..n {
            for k in 0..n {
                let p = [
                    (i as f64 + 0.5) * s,
                    (j as f64 + 0.5) * s,
                    (k as f64 + 0.5) * s,
                ];
                if lattice_field(family, FillMode::Network, cell, param, p).v > 0.0 {
                    d[(i * n + j) * n + k] = 0.0;
                }
            }
        }
    }
    // Chamfer weights per offset length: 1 (√1), √2, √3. Pass 1 scans ascending with the
    // 13 lex-earlier neighbours, pass 2 descending with the 13 lex-later ones.
    let mut later = Vec::new();
    for dx in -1..=1i32 {
        for dy in -1..=1i32 {
            for dz in -1..=1i32 {
                if (dx, dy, dz) != (0, 0, 0) {
                    let w = ((dx * dx + dy * dy + dz * dz) as f32).sqrt();
                    later.push((dx, dy, dz, w));
                }
            }
        }
    }
    let earlier: Vec<(i32, i32, i32, f32)> =
        later.iter().map(|&(a, b, c, w)| (-a, -b, -c, w)).collect();
    for pass in 0..2usize {
        let range: Vec<usize> = if pass == 0 {
            (0..n).collect()
        } else {
            (0..n).rev().collect()
        };
        let neigh: &[(i32, i32, i32, f32)] = if pass == 0 { &earlier } else { &later };
        for &i in &range {
            for &j in &range {
                for &k in &range {
                    let idx = (i * n + j) * n + k;
                    if d[idx] == 0.0 {
                        continue;
                    }
                    let mut best = d[idx];
                    for &(di, dj, dk, w) in neigh {
                        let (ii, jj, kk) = (i as i32 + di, j as i32 + dj, k as i32 + dk);
                        if ii < 0
                            || jj < 0
                            || kk < 0
                            || ii >= n as i32
                            || jj >= n as i32
                            || kk >= n as i32
                        {
                            continue;
                        }
                        let q = ((ii * n as i32 + jj) * n as i32 + kk) as usize;
                        best = best.min(d[q] + w);
                    }
                    d[idx] = best;
                }
            }
        }
    }
    // Local maxima of the chamfer field over material, all 26 neighbours in bounds.
    let mut found = 0usize;
    let mut best = f32::INFINITY;
    for i in 1..n - 1 {
        for j in 1..n - 1 {
            for k in 1..n - 1 {
                let idx = (i * n + j) * n + k;
                let dv = d[idx];
                if dv == 0.0 || dv >= 1.0e5 {
                    continue;
                }
                let mut is_max = true;
                for di in -1..=1i32 {
                    for dj in -1..=1i32 {
                        for dk in -1..=1i32 {
                            if (di, dj, dk) == (0, 0, 0) {
                                continue;
                            }
                            let q = (((i as i32 + di) * n as i32 + (j as i32 + dj)) * n as i32
                                + (k as i32 + dk)) as usize;
                            if d[q] > dv {
                                is_max = false;
                            }
                        }
                    }
                }
                if is_max {
                    found += 1;
                    best = best.min(dv);
                }
            }
        }
    }
    if found == 0 {
        // Degenerate (no strut centre inside the sample): fall back to half the widest reading.
        let mt = d.iter().cloned().fold(0.0f32, f32::max);
        return (0.5 * mt) as f64 * s;
    }
    best as f64 * s
}

/// Volume-weighted least-squares slope of ux against x over the P2 elements whose corner
/// centroid lies in [lo, hi]. The interior lattice state is affine in x, so this recovers the
/// macro strain from the material that is actually present (walls), void included nowhere.
/// Returns None when the band holds no elements or the band is degenerate.
pub fn strain_slope(mesh: &P2Mesh, u: &[f64], lo: f64, hi: f64) -> Option<f64> {
    let (mut sw, mut sx, mut su, mut sxx, mut sxu) = (0.0f64, 0.0, 0.0, 0.0, 0.0);
    for (e, t) in mesh.tets.iter().enumerate() {
        let mut cx = 0.0f64;
        let mut uc = 0.0f64;
        for k in 0..4 {
            cx += mesh.nodes[t[k] as usize][0];
            uc += u[3 * t[k] as usize];
        }
        cx /= 4.0;
        uc /= 4.0;
        if cx < lo || cx > hi {
            continue;
        }
        let v = mesh.tet_volume(e).abs();
        sw += v;
        sx += cx * v;
        su += uc * v;
        sxx += cx * cx * v;
        sxu += cx * uc * v;
    }
    if sw <= 0.0 {
        return None;
    }
    let xbar = sx / sw;
    let ubar = su / sw;
    let num = sxu - sw * xbar * ubar;
    let den = sxx - sw * xbar * xbar;
    if den < 1e-12 * sxx + 1e-14 {
        return None;
    }
    Some(num / den)
}

/// One coupon's settings.
#[derive(Clone, Debug)]
pub struct CouponSpec {
    pub family: Family,
    pub mode: FillMode,
    /// Nominal target density (the meshed density is reported as `rho_mesh`).
    pub rho: f64,
    /// Unit cell size (mm).
    pub cell: f64,
    /// Cells per side.
    pub n: usize,
    /// Mesh size in units of the mode's thin feature (see [`CouponResult::feature`]).
    pub hmul: f64,
    /// Lattice pattern offset inside the coupon (mm).
    pub shift: f64,
    /// Explicit mesh size; `None` uses `hmul·feature`.
    pub h: Option<f64>,
    /// Interior strain band in cells (each side).
    pub band: f64,
    /// Solid modulus of the coupon material (MPa).
    pub young: f64,
    pub nu: f64,
    /// Applied traction magnitude on the x = L platen (MPa).
    pub sigma: f64,
}

impl CouponSpec {
    /// The sweep's settings: cell 2, band 1, σ 10, 1000 MPa coupon material, shift and mesh
    /// size unset (h from `hmul`).
    pub fn new(family: Family, mode: FillMode, rho: f64) -> Self {
        CouponSpec {
            family,
            mode,
            rho,
            cell: 2.0,
            n: 3,
            hmul: default_hmul(mode),
            shift: 0.0,
            h: None,
            band: 1.0,
            young: 1000.0,
            nu: 0.3,
            sigma: 10.0,
        }
    }
}

/// Default mesh size in feature units: sheet walls are flat (one mesh cell per wall resolves
/// them, ratios 0.95–0.99 measured); network struts are curved and partly eaten at one cell per
/// diameter (measured ρ_mesh 0.737 of nominal and a solver refusal on the degraded gyroid ρ = 0.2
/// mesh), so one cell per strut radius.
pub fn default_hmul(mode: FillMode) -> f64 {
    match mode {
        FillMode::Network => 0.5,
        FillMode::Sheet => 1.0,
    }
}

/// One coupon run: settings, measured numbers and the run's diagnostic lines (the sweep log's
/// per-coupon block, in order).
#[derive(Clone, Debug)]
pub struct CouponResult {
    /// Calibration parameter actually used (sheet: wall thickness t; network: level offset).
    pub t_param: f64,
    /// The mode's thin feature the mesh resolves: sheet `t`, network `2R` (mm).
    pub feature: f64,
    /// Mesh size used (mm).
    pub h: f64,
    pub verts: usize,
    pub tets: usize,
    pub dofs: usize,
    /// Achieved volume fraction: meshed material volume over L³.
    pub rho_mesh: f64,
    pub min_dihedral: f64,
    pub max_dihedral: f64,
    pub mesh_secs: f64,
    pub solve_secs: f64,
    pub iters: usize,
    pub converged: bool,
    /// Macroscopic section stress Σ (MPa).
    pub macro_stress: f64,
    pub eps_int: Option<f64>,
    pub eps_full: Option<f64>,
    /// Apparent modulus over the interior band (MPa).
    pub e_int: Option<f64>,
    /// Apparent modulus over the whole coupon (MPa): the boundary-layer diagnostic.
    pub e_full: Option<f64>,
    /// Volume-weighted interior mean σxx (MPa).
    pub sxx_int: f64,
    /// Volume-weighted σxx over the whole coupon (MPa).
    pub sxx_all: f64,
    /// Equilibrium identity error ∫σxx dV vs Σ Fₓ·x (%).
    pub ident_energy_err: f64,
    /// Equilibrium identity error ⟨σxx⟩ₘₐₜ vs −Σ/(ρ·L²) (%).
    pub ident_hill_err: f64,
    /// Planar load-patch area (mm²; the tilted patches are excluded).
    pub area_planar: f64,
    /// All +x-normal-patch area including the tilted transition triangles (mm²).
    pub area_all: f64,
    /// Connected components after the mesh (1 = nothing dropped).
    pub n_components: usize,
    /// Tets dropped with the smaller components.
    pub dropped_tets: usize,
    /// The run's diagnostic lines, in print order.
    pub log: Vec<String>,
}

fn dot(x: V3, y: V3) -> f64 {
    x[0] * y[0] + x[1] * y[1] + x[2] * y[2]
}

/// Index of the mesh node nearest `target` among those passing `filter`.
fn nearest(mesh: &P2Mesh, target: V3, filter: &dyn Fn(V3) -> bool) -> Option<usize> {
    let mut best = (0usize, f64::MAX);
    for (i, p) in mesh.nodes.iter().enumerate() {
        if !filter(*p) {
            continue;
        }
        let d2 = dot(
            [p[0] - target[0], p[1] - target[1], p[2] - target[2]],
            [p[0] - target[0], p[1] - target[1], p[2] - target[2]],
        );
        if d2 < best.1 {
            best = (i, d2);
        }
    }
    if best.1.is_finite() {
        Some(best.0)
    } else {
        None
    }
}

/// Keep only the largest face-connected component of a tet mesh; returns the number of
/// components and the dropped tet count. Floating fragments cannot carry load and are not
/// supported by the solver's fixture, so a mesh that would contain them must be pruned (V7's
/// resolved-lattice beam uses this too).
pub fn keep_largest_connected(lin: &mut TetMesh) -> (usize, usize) {
    let mut comp = vec![usize::MAX; lin.tets.len()];
    let mut ncomp = 0usize;
    let mut faces: std::collections::HashMap<[u32; 3], Vec<usize>> =
        std::collections::HashMap::new();
    for (e, t) in lin.tets.iter().enumerate() {
        for fi in 0..4 {
            let mut fc = [t[(fi + 1) % 4], t[(fi + 2) % 4], t[(fi + 3) % 4]];
            fc.sort();
            faces.entry(fc).or_default().push(e);
        }
    }
    let mut stack = Vec::new();
    for e0 in 0..comp.len() {
        if comp[e0] != usize::MAX {
            continue;
        }
        comp[e0] = ncomp;
        stack.push(e0);
        while let Some(e) = stack.pop() {
            let t = lin.tets[e];
            for fi in 0..4 {
                let mut fc = [t[(fi + 1) % 4], t[(fi + 2) % 4], t[(fi + 3) % 4]];
                fc.sort();
                for &e2 in &faces[&fc] {
                    if comp[e2] == usize::MAX {
                        comp[e2] = ncomp;
                        stack.push(e2);
                    }
                }
            }
        }
        ncomp += 1;
    }
    if ncomp > 1 {
        let mut sizes = vec![0usize; ncomp];
        for &c in &comp {
            sizes[c] += 1;
        }
        let keep = (0..ncomp).max_by_key(|&c| sizes[c]).expect("ncomp > 1");
        let dropped = lin.tets.len() - sizes[keep];
        let mut vmap = vec![u32::MAX; lin.verts.len()];
        let mut verts = Vec::<V3>::with_capacity(lin.verts.len());
        for (e, t) in lin.tets.iter().enumerate() {
            if comp[e] != keep {
                continue;
            }
            for &v in t {
                if vmap[v as usize] == u32::MAX {
                    vmap[v as usize] = verts.len() as u32;
                    verts.push(lin.verts[v as usize]);
                }
            }
        }
        let tets: Vec<_> = lin
            .tets
            .iter()
            .enumerate()
            .filter(|(e, _)| comp[*e] == keep)
            .map(|(_, t)| {
                [
                    vmap[t[0] as usize],
                    vmap[t[1] as usize],
                    vmap[t[2] as usize],
                    vmap[t[3] as usize],
                ]
            })
            .collect();
        lin.verts = verts;
        lin.tets = tets;
        (ncomp, dropped)
    } else {
        (ncomp, 0)
    }
}

/// Mesh, load, fixture, solve and measure one TPMS coupon. Panics when the solver refuses the
/// study (the caller's run aborts; the sweep loop is per-invocation).
pub fn run(spec: &CouponSpec) -> CouponResult {
    let mut log = Vec::new();
    let cell = spec.cell;
    let l = spec.n as f64 * cell;
    let cal = calibration(spec.family, spec.mode);
    let t = cal.param_for(spec.rho, cell);
    let feature = match spec.mode {
        FillMode::Sheet => t,
        FillMode::Network => 2.0 * strut_min_radius(spec.family, cell, t),
    };
    let h = spec.h.unwrap_or(feature * spec.hmul);
    let dinfo = match spec.mode {
        FillMode::Sheet => format!("t={t:.5}, hmul={}", spec.hmul),
        FillMode::Network => format!("R={:.5}, hmul={}", 0.5 * feature, spec.hmul),
    };
    log.push(format!(
        "coupon: family={:?} mode={:?} rho={} cell={cell} n={} h={h:.5} ({dinfo}, shift={})",
        spec.family, spec.mode, spec.rho, spec.n, spec.shift
    ));

    let param = t;
    let shift = spec.shift;
    let f = |p: V3| {
        let q = [p[0] + shift, p[1] + shift, p[2] + shift];
        box_sdf(p, l).max(lattice_field(spec.family, spec.mode, cell, param, q).v)
    };
    let t0 = Instant::now();
    let (mut lin, _) = stuff(f, [0.0; 3], [l; 3], h);
    lin.compact();
    let (n_components, dropped_tets) = keep_largest_connected(&mut lin);
    if n_components > 1 {
        log.push(format!(
            "components: {n_components} (largest {:.1}% of tets); dropped {dropped_tets} tets ({:.1}%) as unconnected fragments",
            100.0 * (lin.tets.len() as f64) / (lin.tets.len() as f64 + dropped_tets as f64),
            100.0 * dropped_tets as f64 / (lin.tets.len() as f64 + dropped_tets as f64),
        ));
    }
    let mesh_secs = t0.elapsed().as_secs_f64();
    let rho_mesh = lin.volume() / (l * l * l);
    let q = quality(&lin);
    log.push(format!(
        "mesh: verts {} tets {} rho_mesh {:.4} (nominal {}, ratio {:.3}) dihedral {:.1}..{:.1} deg, {:.1}s",
        lin.verts.len(),
        lin.tets.len(),
        rho_mesh,
        spec.rho,
        rho_mesh / spec.rho,
        q.min_dihedral_deg,
        q.max_dihedral_deg,
        mesh_secs
    ));
    let mesh = P2Mesh::from_linear(&lin.verts, &lin.tets);
    log.push(format!("dofs {}", mesh.ndofs()));

    // ---- boundary conditions -----------------------------------------------------------------
    let nn = mesh.nodes.len();
    let mut fixed = vec![0u8; nn];
    for (i, p) in mesh.nodes.iter().enumerate() {
        if p[0].abs() < 1e-6 {
            fixed[i] |= 1; // ux = 0 on the x = 0 platen
        }
    }
    // Rigid-mode fixture: the free-slip platens leave ty, tz and the rotations. Kill them with
    // two point constraints placed on the coupon boundary, ≥ 1 cell from the strain probe band
    // (the largest component is kept above, so the P2 mesh is one piece):
    //   A ≈ the (0,0,0) corner: uy = uz = 0  (kills translations; B kills the three rotations)
    //   B: a node on the y = 0 face near the far x edge with z ≈ cell/2: uy = 0.
    let node_a = nearest(&mesh, [0.0; 3], &|_| true).expect("a node exists");
    fixed[node_a] |= 0b110; // uy, uz
    let node_b = nearest(&mesh, [l, 0.0, cell], &|p: V3| {
        p[1].abs() < 1e-6 && p[0] > l - 2.0 * h
    })
    .expect("a node on the y=0 face near the far x edge");
    fixed[node_b] |= 0b010; // uy
    log.push(format!(
        "fixture: A={:?} uy+uz, B={:?} uy (nodes {} of {})",
        mesh.nodes[node_a].map(|c| c as f32),
        mesh.nodes[node_b].map(|c| c as f32),
        node_a,
        node_b
    ));
    let boundary = lin.boundary();
    let facets: Vec<fieldsmith_core::regions::Facet> = boundary
        .iter()
        .map(|(fc, ti)| {
            let tet = &mesh.tets[*ti as usize];
            let local = fc.map(|v| {
                tet[..4]
                    .iter()
                    .position(|&x| x == v)
                    .expect("face vertex in tet")
            });
            fieldsmith_core::regions::Facet {
                nodes: face_nodes(tet, local),
            }
        })
        .collect();
    let mut force = vec![0.0; nn * 3];
    let mut area_normal = 0.0;
    let mut area_nodes = 0.0;
    for fct in &facets {
        let nrm = fct.normal(&mesh);
        let at_x = fct
            .nodes
            .iter()
            .all(|&n| mesh.nodes[n as usize][0] > l - 1e-6);
        let area = fct.area(&mesh);
        // +x face: load the exact-planar patches only (all six nodes at x = L, normal +1).
        // Tilted transition triangles at the wall-boundary curve are NOT at x = L; loading them
        // at their full area applies 1/n_x too much axial force and breaks the σxx-volume
        // equilibrium identity (measured: normal filter 1.55 vs planar 0.82, σxx̄ off by 27 %).
        let is_load = at_x && nrm[0] > 0.99;
        if is_load {
            area_nodes += area;
            for &n in &fct.nodes[3..] {
                force[3 * n as usize] += -spec.sigma * area / 3.0;
            }
        }
        if nrm[0] > 0.99 {
            area_normal += area;
        }
    }
    log.push(format!(
        "loaded +x area: planar {area_nodes:.4} (was {area_normal:.4} incl. tilted) (cross-section {:.4})",
        l * l
    ));
    if area_nodes == 0.0 {
        // Nothing loaded: the RESULT of such a run is meaningless (trivial solve, NaN E*).
        // Diagnose where the mesh and the field disagree with the face plane.
        let max_x = mesh.nodes.iter().map(|p| p[0]).fold(-f64::MAX, f64::max);
        let mut fmin = f64::MAX;
        let mut fmax = -f64::MAX;
        let mut in_mat = 0usize;
        let mut total = 0usize;
        for i in 0..48 {
            for j in 0..48 {
                let y = i as f64 / 47.0 * l;
                let z = j as f64 / 47.0 * l;
                let v = f([l, y, z]);
                fmin = fmin.min(v);
                fmax = fmax.max(v);
                total += 1;
                if v <= 1e-9 {
                    in_mat += 1;
                }
            }
        }
        log.push(format!(
            "diagnostic: no planar +x patches: field on face min {fmin:.4} max {fmax:.4}, material fraction of face {}/{} = {:.4}, max node x {max_x:.6} (l = {l})",
            in_mat,
            total,
            in_mat as f64 / total as f64
        ));
        let mut vert_at = 0usize;
        let mut mid_at = 0usize;
        for (i, p) in mesh.nodes.iter().enumerate() {
            if p[0] > l - 1e-6 {
                if i < mesh.nverts {
                    vert_at += 1;
                } else {
                    mid_at += 1;
                }
            }
        }
        let mut cplanar = 0usize;
        let mut cplanar_area = 0.0;
        let mut mid_dev = 0.0f64;
        for fct in &facets {
            let cs = [
                mesh.nodes[fct.nodes[0] as usize][0],
                mesh.nodes[fct.nodes[1] as usize][0],
                mesh.nodes[fct.nodes[2] as usize][0],
            ];
            if cs.iter().all(|&x| x > l - 1e-6) {
                cplanar += 1;
                cplanar_area += fct.area(&mesh);
                for &n in &fct.nodes[3..] {
                    let d = (mesh.nodes[n as usize][0] - l).abs();
                    if d > mid_dev {
                        mid_dev = d;
                    }
                }
            }
        }
        log.push(format!(
            "diagnostic: verts at x≈l: {vert_at}, edge-mids: {mid_at}; corner-planar boundary facets: {cplanar} (area {cplanar_area:.4}), max midside |x−l| among them: {mid_dev:.2e}"
        ));
    }

    let macro_stress = force.iter().step_by(3).sum::<f64>().abs() / (l * l);
    // Σ F_x·x over all nodes. With the test field w = x·e_x (admissible: w vanishes on the
    // ux = 0 platen, and the point fixtures react only in y/z), FE equilibrium gives the
    // identity ∫_Ω σxx dV = Σ F_x·x, checked below against the element-stress integral.
    let sum_fxx: f64 = (0..nn).map(|i| force[3 * i] * mesh.nodes[i][0]).sum();
    let mat = Material {
        young: vec![spec.young; mesh.tets.len()],
        poisson: spec.nu,
    };
    let problem = Problem {
        mesh: mesh.clone(),
        material: mat.clone(),
        fixed,
        force,
    };
    let t0 = Instant::now();
    let (u, rep) = match solve(&problem, SolverOptions::default()) {
        Ok((u, rep)) => (u, rep),
        Err(e) => {
            // Dump the collected diagnostics under the panic, so a refused solve can be
            // diagnosed from the log alone.
            for line in &log {
                eprintln!("{line}");
            }
            panic!("P2 solve: {e}");
        }
    };
    let solve_secs = t0.elapsed().as_secs_f64();
    log.push(format!(
        "solve: iters {} rel_residual {:.3e} converged {} ({:.1}s)",
        rep.iterations, rep.rel_residual, rep.converged, solve_secs
    ));

    // ---- macro strain ------------------------------------------------------------------------
    // ε from the volume-weighted ux slope over the interior band (x = band·cell … L − band·cell),
    // plus a full-coupon slope as the boundary-layer diagnostic. Wider bands shed platen contact
    // artefacts (sparse contact through the first stratum) but average over more of the periodic
    // microstructure; the measured band drift is reported per family.
    let band = spec.band * cell;
    let eps_int = if l - band > band + 1e-9 {
        strain_slope(&mesh, &u, band, l - band)
    } else {
        None // band too wide: the interior band degenerates
    };
    let eps_full = strain_slope(&mesh, &u, 0.0, l);
    let e_star_int = eps_int.map(|e| macro_stress / e.abs());
    let e_star_full = eps_full.map(|e| macro_stress / e.abs());

    // ---- interior mean σxx (volume-weighted element average) ----------------------------------
    let rd_xi = {
        // The mean of the element's nodal barycentrics is the element centroid, (0.25, …) for a
        // P2 tet: the divisor is the node COUNT, however many nodes the element carries.
        let pts = P2Tetrahedron.nodal_coordinates();
        let mut c = [0.0f64; 3];
        for xi in pts.iter() {
            for (j, cj) in c.iter_mut().enumerate() {
                *cj += xi[j] / pts.len() as f64;
            }
        }
        c
    };
    let mut sxx = 0.0;
    let mut vol = 0.0;
    let mut sxx_all = 0.0;
    let mut vol_all = 0.0;
    for e in 0..mesh.tets.len() {
        let tt = &mesh.tets[e];
        let mut c = [0.0f64; 3];
        for &v in tt.iter() {
            let n = mesh.nodes[v as usize];
            for (j, cj) in c.iter_mut().enumerate() {
                *cj += n[j] / 4.0;
            }
        }
        let interior = c[0] >= cell
            && c[0] <= l - cell
            && c[1] >= cell
            && c[1] <= l - cell
            && c[2] >= cell
            && c[2] <= l - cell;
        let s = element_stress(&mesh, &u, e, spec.young, spec.nu, rd_xi);
        let v = mesh.tet_volume(e).abs();
        if interior {
            sxx += s[0] * v;
            vol += v;
        }
        sxx_all += s[0] * v;
        vol_all += v;
    }
    log.push(format!(
        "interior elements: sigma_xx {:.4} MPa (vol-weighted, {} mm3); whole coupon sigma_xx {:.4} (macro stress {:.4})",
        sxx / vol,
        vol,
        sxx_all / vol_all,
        macro_stress
    ));
    // Equilibrium identities, logged instead of hand-checked. Quadrature note: the P2
    // gradient is degree-1 in the barycentrics (affine element map), so σxx is degree-1 and
    // its element average is EXACT at the tet centroid — the loop above is exact integration.
    let err_int = 100.0 * (sxx_all - sum_fxx).abs() / sum_fxx.abs().max(1e-30);
    log.push(format!(
        "identity: ∫σxx dV = {sxx_all:.4} N·mm vs Σ F_x·x = {sum_fxx:.4} N·mm (err {err_int:.3}%)"
    ));
    let hill = sxx_all / vol_all; // = −Σ/ρ over the material
    let expect = -macro_stress / rho_mesh;
    let err_hill = 100.0 * (hill - expect).abs() / expect.abs().max(1e-30);
    log.push(format!(
        "identity: ⟨σxx⟩_mat = {hill:.5} vs −Σ/L²/ρ = {expect:.5} (err {err_hill:.3}%)"
    ));
    log.push(format!(
        "strain: eps_int {} (band x = {band} ... {}), eps_full {:.5e}",
        eps_int
            .map(|e| format!("{e:.5e}"))
            .unwrap_or_else(|| "n/a (N=2)".into()),
        l - band,
        eps_full.unwrap_or(f64::NAN)
    ));
    log.push(format!(
        "RESULT family={:?} mode={:?} rho={} cell={} n={} hmul={} band={} dofs={} rho_mesh={:.5} E_int={} E_full={} iters={} secs={:.1}",
        spec.family,
        spec.mode,
        spec.rho,
        cell,
        spec.n,
        spec.hmul,
        spec.band,
        mesh.ndofs(),
        rho_mesh,
        e_star_int.map(|e| format!("{e:.2}")).unwrap_or_else(|| "n/a".into()),
        e_star_full.map(|e| format!("{e:.2}")).unwrap_or_else(|| "n/a".into()),
        rep.iterations,
        mesh_secs + solve_secs
    ));

    CouponResult {
        t_param: t,
        feature,
        h,
        verts: lin.verts.len(),
        tets: lin.tets.len(),
        dofs: mesh.ndofs(),
        rho_mesh,
        min_dihedral: q.min_dihedral_deg,
        max_dihedral: q.max_dihedral_deg,
        mesh_secs,
        solve_secs,
        iters: rep.iterations,
        converged: rep.converged,
        macro_stress,
        eps_int,
        eps_full,
        e_int: e_star_int,
        e_full: e_star_full,
        sxx_int: sxx / vol,
        sxx_all: sxx_all / vol_all,
        ident_energy_err: err_int,
        ident_hill_err: err_hill,
        area_planar: area_nodes,
        area_all: area_normal,
        n_components,
        dropped_tets,
        log,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_tet_verts() -> Vec<V3> {
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ]
    }

    /// Two P2 tets at distinct x (centroids 0.25 and 1.25), so the least-squares slope is
    /// well posed (single-x bands are degenerate: num = den = 0).
    fn two_tet_mesh() -> P2Mesh {
        let verts: Vec<V3> = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 0.0, 1.0],
        ];
        P2Mesh::from_linear(&verts, &[[0, 1, 2, 3], [4, 5, 6, 7]])
    }

    #[test]
    fn strain_slope_is_exact_for_a_linear_field() {
        let mesh = two_tet_mesh();
        // u = 2·x everywhere (edge midsides included) → slope 2 exactly.
        let u: Vec<f64> = mesh
            .nodes
            .iter()
            .flat_map(|p| [2.0 * p[0], 0.0, 0.0])
            .collect();
        let s = strain_slope(&mesh, &u, 0.0, 2.0).expect("both tets in band");
        assert!((s - 2.0).abs() < 1e-6, "slope {s}");
        // Sign carries: u = −3·x → slope −3.
        let u: Vec<f64> = mesh
            .nodes
            .iter()
            .flat_map(|p| [-3.0 * p[0], 0.0, 0.0])
            .collect();
        let s = strain_slope(&mesh, &u, 0.0, 2.0).expect("both tets in band");
        assert!((s + 3.0).abs() < 1e-6, "slope {s}");
        // A band with only one x-value (the first tet) is degenerate → None.
        assert!(strain_slope(&mesh, &u, 0.2, 0.3).is_none());
        // A band with no element centroids returns None rather than a number.
        assert!(strain_slope(&mesh, &u, 3.0, 4.0).is_none());
    }

    #[test]
    fn keep_largest_keeps_the_face_connected_group() {
        // Three face-connected tets (a chain through shared faces) plus one isolated tet
        // sharing no vertex with them at all.
        let verts: Vec<V3> = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
            [100.0, 0.0, 0.0],
            [101.0, 0.0, 0.0],
            [100.0, 1.0, 0.0],
            [100.0, 0.0, 1.0],
        ];
        let mut lin = TetMesh {
            verts,
            tets: vec![[0, 1, 2, 3], [1, 2, 3, 4], [2, 3, 4, 5], [6, 7, 8, 9]],
        };
        let (ncomp, dropped) = keep_largest_connected(&mut lin);
        assert_eq!((ncomp, dropped), (2, 1));
        assert_eq!(lin.tets.len(), 3);
        assert_eq!(lin.verts.len(), 6);
        // Single-component meshes are returned untouched.
        let mut lin = TetMesh {
            verts: unit_tet_verts(),
            tets: vec![[0, 1, 2, 3]],
        };
        assert_eq!(keep_largest_connected(&mut lin), (1, 0));
        assert_eq!(lin.tets, vec![[0, 1, 2, 3]]);
    }
}
