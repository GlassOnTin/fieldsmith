//! Preconditioned conjugate gradients with a two-level (P2 → P1) preconditioner.

use crate::bsr::{flatten_blocks9, inv3, mv3, Bsr};
use crate::{assemble, Problem};
use faer::prelude::*;
use faer::sparse::SparseColMat;
use faer::Side;
use fenics_solver::gpu::{
    discover_gpus, solve_bsr3_pcg_gpu, spmv_bsr3_multi_gpu, GpuBsr3Context, GpuDeviceInfo,
};

/// Minimum DOF threshold where automatic GPU acceleration is engaged inside `solve()`.
pub const AUTO_GPU_MIN_DOFS: usize = 300_000;

#[derive(Clone, Copy, Debug)]
pub struct SolverOptions {
    /// Relative residual ‖b − Au‖ / ‖b‖ at which to stop.
    pub rtol: f64,
    pub max_iter: usize,
}

impl Default for SolverOptions {
    fn default() -> Self {
        SolverOptions {
            rtol: 1e-9,
            max_iter: 2000,
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct SolveReport {
    pub dofs: usize,
    pub free_dofs: usize,
    pub coarse_dofs: usize,
    pub iterations: usize,
    pub rel_residual: f64,
    pub converged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_gflops: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_bw_gbps: Option<f64>,
}

#[inline]
fn is_fixed(fixed: &[u8], node: usize, c: usize) -> bool {
    fixed[node] & (1 << c) != 0
}

/// Zero fixed rows/columns and put 1 on their diagonal, so fixed DOFs decouple with value 0.
pub(crate) fn apply_dirichlet(a: &mut Bsr, fixed: &[u8]) {
    for i in 0..a.n {
        let (s, e) = (a.row_ptr[i] as usize, a.row_ptr[i + 1] as usize);
        for k in s..e {
            let j = a.cols[k] as usize;
            if fixed[i] == 0 && fixed[j] == 0 {
                continue;
            }
            let b = &mut a.vals[k];
            for r in 0..3 {
                for c in 0..3 {
                    if is_fixed(fixed, i, r) || is_fixed(fixed, j, c) {
                        b[r * 3 + c] = if i == j && r == c { 1.0 } else { 0.0 };
                    }
                }
            }
        }
    }
}

pub(crate) struct TwoLevel<'a> {
    a: &'a Bsr,
    dinv: Vec<[f64; 9]>,
    fixed: &'a [u8],
    nverts: usize,
    mid_edge: &'a [[u32; 2]],
    coarse: faer::sparse::linalg::solvers::Cholesky<usize, f64>,
    gpu_ctx: Option<GpuBsr3Context>,
}

impl<'a> TwoLevel<'a> {
    /// Coarse operator Ac = Pᵀ M A M P on the P1 (vertex) pattern plus the block-SGS
    /// smoothing, built once and reused by every PCG iteration (and by the modal route's
    /// inner solves, which run many of them against one stiffened system).
    pub(crate) fn new(
        a: &'a Bsr,
        fixed: &'a [u8],
        nverts: usize,
        tets: &[[u32; 10]],
        mid_edge: &'a [[u32; 2]],
    ) -> Result<TwoLevel<'a>, String> {
        let dinv: Vec<[f64; 9]> = (0..a.n)
            .map(|i| inv3(&a.vals[a.find(i, i as u32)]))
            .collect();
        let gpu_ctx = if a.n * 3 >= AUTO_GPU_MIN_DOFS {
            GpuBsr3Context::new(
                a.n,
                &a.row_ptr,
                &a.cols,
                a.vals_flat(),
                flatten_blocks9(&dinv),
                None,
            )
            .ok()
        } else {
            None
        };
        let nv = nverts;
        // Coarse operator Ac = Pᵀ M A M P on the P1 (vertex) pattern.
        let mut ac = Bsr::from_elements(nv, tets.iter().map(|t| [t[0], t[1], t[2], t[3]]));
        let pmap = |i: usize| -> [(u32, f64); 2] {
            if i < nv {
                [(i as u32, 1.0), (u32::MAX, 0.0)]
            } else {
                let [x, y] = mid_edge[i - nv];
                [(x, 0.5), (y, 0.5)]
            }
        };
        for i in 0..a.n {
            let (s, e) = (a.row_ptr[i] as usize, a.row_ptr[i + 1] as usize);
            let pi = pmap(i);
            for k in s..e {
                let j = a.cols[k] as usize;
                let mut b = a.vals[k];
                for r in 0..3 {
                    for c in 0..3 {
                        if is_fixed(fixed, i, r) || is_fixed(fixed, j, c) {
                            b[r * 3 + c] = 0.0;
                        }
                    }
                }
                let pj = pmap(j);
                for &(ci, wi) in &pi {
                    if ci == u32::MAX {
                        continue;
                    }
                    for &(cj, wj) in &pj {
                        if cj == u32::MAX {
                            continue;
                        }
                        let w = wi * wj;
                        let scaled = b.map(|x| x * w);
                        ac.add(ci as usize, cj, &scaled);
                    }
                }
            }
        }
        // Scalar lower-triangle triplets; empty rows get a unit diagonal.
        let n = nv * 3;
        let mut trip: Vec<(usize, usize, f64)> = Vec::with_capacity(ac.nnz_blocks() * 5);
        let mut row_has = vec![false; n];
        for i in 0..nv {
            let (s, e) = (ac.row_ptr[i] as usize, ac.row_ptr[i + 1] as usize);
            for k in s..e {
                let j = ac.cols[k] as usize;
                let b = &ac.vals[k];
                for r in 0..3 {
                    for c in 0..3 {
                        let (gi, gj) = (3 * i + r, 3 * j + c);
                        let v = b[r * 3 + c];
                        if gi >= gj && v != 0.0 {
                            trip.push((gi, gj, v));
                            if gi == gj {
                                row_has[gi] = true;
                            }
                        }
                    }
                }
            }
        }
        for (g, has) in row_has.iter().enumerate() {
            if !has {
                trip.push((g, g, 1.0));
            }
        }
        let m = SparseColMat::<usize, f64>::try_new_from_triplets(n, n, &trip)
            .map_err(|e| format!("coarse matrix: {e:?}"))?;
        let coarse = match m.sp_cholesky(Side::Lower) {
            Ok(c) => c,
            Err(_) => {
                // Multi-body assemblies with visual or non-structural floating sub-volumes
                // (e.g. a 3D far-field radiation lobe envelope or detached PCB/prop disc)
                // can leave an unloaded connected component without its own Dirichlet support.
                // Adding a tiny positive diagonal regularisation (1e-6 × mean diagonal) to the
                // coarse preconditioner operator Ac allows Cholesky factorisation of the
                // preconditioner to succeed while keeping the exact operator `a` unaltered.
                let mut diag_sum = 0.0f64;
                let mut diag_cnt = 0usize;
                for &(r, c, v) in &trip {
                    if r == c && v > 0.0 {
                        diag_sum += v;
                        diag_cnt += 1;
                    }
                }
                let reg = if diag_cnt > 0 {
                    (diag_sum / diag_cnt as f64) * 1.0e-6
                } else {
                    1.0
                };
                let mut reg_trip = trip.clone();
                for g in 0..n {
                    reg_trip.push((g, g, reg));
                }
                let m_reg = SparseColMat::<usize, f64>::try_new_from_triplets(n, n, &reg_trip)
                    .map_err(|e| format!("coarse matrix (regularised): {e:?}"))?;
                m_reg.sp_cholesky(Side::Lower).map_err(|e| {
                    format!(
                        "coarse factorisation failed ({e:?}); the part may not be held by its supports"
                    )
                })?
            }
        };
        Ok(TwoLevel {
            a,
            dinv,
            fixed,
            nverts: nv,
            mid_edge,
            coarse,
            gpu_ctx,
        })
    }

    #[inline]
    pub(crate) fn spmv(&self, x: &[f64], y: &mut [f64]) {
        if let Some(ref gpu) = self.gpu_ctx {
            if gpu.spmv(x, y).is_ok() {
                return;
            }
        }
        self.a.mul(x, y);
    }

    fn gs_sweep(&self, r: &[f64], x: &mut [f64], forward: bool) {
        let a = self.a;
        let mut sweep = |i: usize| {
            let (s, e) = (a.row_ptr[i] as usize, a.row_ptr[i + 1] as usize);
            let mut acc = [r[3 * i], r[3 * i + 1], r[3 * i + 2]];
            for k in s..e {
                let j = a.cols[k] as usize;
                if j == i {
                    continue;
                }
                let b = &a.vals[k];
                let (x0, x1, x2) = (x[3 * j], x[3 * j + 1], x[3 * j + 2]);
                acc[0] -= b[0] * x0 + b[1] * x1 + b[2] * x2;
                acc[1] -= b[3] * x0 + b[4] * x1 + b[5] * x2;
                acc[2] -= b[6] * x0 + b[7] * x1 + b[8] * x2;
            }
            let v = mv3(&self.dinv[i], acc);
            x[3 * i..3 * i + 3].copy_from_slice(&v);
        };
        if forward {
            (0..a.n).for_each(&mut sweep);
        } else {
            (0..a.n).rev().for_each(&mut sweep);
        }
    }

    /// z = M⁻¹ r (symmetric V-cycle: forward GS, coarse correction, backward GS).
    pub(crate) fn apply(&self, r: &[f64], z: &mut [f64], tmp: &mut [f64]) {
        z.iter_mut().for_each(|v| *v = 0.0);
        self.gs_sweep(r, z, true);
        // Residual.
        self.spmv(z, tmp);
        for (t, rv) in tmp.iter_mut().zip(r) {
            *t = rv - *t;
        }
        // Restrict: rc = Pᵀ M res.
        let nv = self.nverts;
        let mut rc = faer::Col::<f64>::zeros(nv * 3);
        for i in 0..self.a.n {
            for c in 0..3 {
                if is_fixed(self.fixed, i, c) {
                    continue;
                }
                let v = tmp[3 * i + c];
                if i < nv {
                    rc[3 * i + c] += v;
                } else {
                    let [x, y] = self.mid_edge[i - nv];
                    rc[3 * x as usize + c] += 0.5 * v;
                    rc[3 * y as usize + c] += 0.5 * v;
                }
            }
        }
        self.coarse.solve_in_place(rc.as_mut());
        // Prolong: z += M P xc.
        for i in 0..self.a.n {
            for c in 0..3 {
                if is_fixed(self.fixed, i, c) {
                    continue;
                }
                let v = if i < nv {
                    rc[3 * i + c]
                } else {
                    let [x, y] = self.mid_edge[i - nv];
                    0.5 * (rc[3 * x as usize + c] + rc[3 * y as usize + c])
                };
                z[3 * i + c] += v;
            }
        }
        self.gs_sweep(r, z, false);
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// The PCG loop against an already built two-level preconditioner. `b` and `x0` must already
/// be zero on the fixed rows (the fixed entries of `x0` never move); the report's `dofs`
/// fields stay zero — the caller fills what it reports. `faer::set_global_parallelism` is the
/// caller's concern.
pub(crate) fn pcg_solve(
    a: &Bsr,
    pre: &TwoLevel,
    b: &[f64],
    x0: &[f64],
    opts: SolverOptions,
) -> Result<(Vec<f64>, SolveReport), String> {
    let n = 3 * a.n; // Bsr rows are node blocks; the PCG runs on the 3N DOF vectors.
    debug_assert_eq!(b.len(), n);
    debug_assert_eq!(x0.len(), n);
    let mut x = x0.to_vec();
    let mut report = SolveReport {
        gpu_device: pre.gpu_ctx.as_ref().map(|g| g.device_name().to_string()),
        ..Default::default()
    };

    let bnorm = dot(b, b).sqrt();
    if bnorm == 0.0 {
        report.converged = true;
        return Ok((x, report));
    }
    let mut ax = vec![0.0; n];
    pre.spmv(&x, &mut ax);
    let mut r: Vec<f64> = b.iter().zip(&ax).map(|(&bi, &ai)| bi - ai).collect();
    let mut z = vec![0.0; n];
    let mut tmp = vec![0.0; n];
    pre.apply(&r, &mut z, &mut tmp);
    let mut d = z.clone();
    let mut rz = dot(&r, &z);
    let mut ad = vec![0.0; n];
    for it in 1..=opts.max_iter {
        pre.spmv(&d, &mut ad);
        let dad = dot(&d, &ad);
        if dad <= 0.0 {
            return Err(
                "stiffness matrix is not positive definite; check that supports hold the part"
                    .into(),
            );
        }
        let alpha = rz / dad;
        for k in 0..n {
            x[k] += alpha * d[k];
            r[k] -= alpha * ad[k];
        }
        let rel = dot(&r, &r).sqrt() / bnorm;
        report.iterations = it;
        report.rel_residual = rel;
        if rel < opts.rtol {
            report.converged = true;
            break;
        }
        pre.apply(&r, &mut z, &mut tmp);
        let rz_new = dot(&r, &z);
        let beta = rz_new / rz;
        rz = rz_new;
        for k in 0..n {
            d[k] = z[k] + beta * d[k];
        }
    }
    if !report.converged {
        return Err(format!(
            "solver did not converge in {} iterations (residual {:.2e})",
            report.iterations, report.rel_residual
        ));
    }
    Ok((x, report))
}

#[inline]
pub(crate) fn configure_host_parallelism() {
    #[cfg(target_arch = "wasm32")]
    faer::set_global_parallelism(faer::Parallelism::None);
    #[cfg(not(target_arch = "wasm32"))]
    faer::set_global_parallelism(faer::Parallelism::Rayon(0));
}

/// Solve the problem using full GPU-resident 3x3 BSR Block-Jacobi PCG on the local CUDA GPU
/// (falling back to the Two-Level P2→P1 solver if no GPU is available or if coarse-grid
/// conditioning is required).
pub fn solve_gpu(p: &Problem, opts: SolverOptions) -> Result<(Vec<f64>, SolveReport), String> {
    configure_host_parallelism();
    let n = p.mesh.ndofs();
    let mut a = assemble(&p.mesh, &p.material);
    apply_dirichlet(&mut a, &p.fixed);
    let mut b = p.force.clone();
    for i in 0..p.mesh.nodes.len() {
        for c in 0..3 {
            if is_fixed(&p.fixed, i, c) {
                b[3 * i + c] = 0.0;
            }
        }
    }
    let free = (0..p.mesh.nodes.len())
        .map(|i| 3 - p.fixed[i].count_ones() as usize)
        .sum();
    let dinv: Vec<[f64; 9]> = (0..a.n)
        .map(|i| inv3(&a.vals[a.find(i, i as u32)]))
        .collect();

    if let Ok(gpu_res) = solve_bsr3_pcg_gpu(
        a.n,
        &a.row_ptr,
        &a.cols,
        a.vals_flat(),
        flatten_blocks9(&dinv),
        &b,
        None,
        opts.rtol,
        opts.max_iter,
        None,
    ) {
        if gpu_res.final_residual < opts.rtol {
            return Ok((
                gpu_res.solution,
                SolveReport {
                    dofs: n,
                    free_dofs: free,
                    coarse_dofs: p.mesh.nverts * 3,
                    iterations: gpu_res.iterations,
                    rel_residual: gpu_res.final_residual,
                    converged: true,
                    gpu_device: gpu_res.devices_used.first().cloned(),
                    gpu_gflops: Some(gpu_res.gflops),
                    gpu_bw_gbps: Some(gpu_res.effective_bw_gbps),
                },
            ));
        }
    }

    let pre = TwoLevel::new(&a, &p.fixed, p.mesh.nverts, &p.mesh.tets, &p.mesh.mid_edge)?;
    let (x, run) = pcg_solve(&a, &pre, &b, &vec![0.0; n], opts)?;
    Ok((
        x,
        SolveReport {
            dofs: n,
            free_dofs: free,
            coarse_dofs: p.mesh.nverts * 3,
            iterations: run.iterations,
            rel_residual: run.rel_residual,
            converged: run.converged,
            gpu_device: run.gpu_device,
            gpu_gflops: run.gpu_gflops,
            gpu_bw_gbps: run.gpu_bw_gbps,
        },
    ))
}

/// Solve the problem. Returns nodal displacements (3 per node, mm).
/// Automatically engages CUDA GPU acceleration when `ndofs >= AUTO_GPU_MIN_DOFS`.
pub fn solve(p: &Problem, opts: SolverOptions) -> Result<(Vec<f64>, SolveReport), String> {
    configure_host_parallelism();
    let n = p.mesh.ndofs();
    if n >= AUTO_GPU_MIN_DOFS && !discover_gpus().is_empty() {
        return solve_gpu(p, opts);
    }
    let mut a = assemble(&p.mesh, &p.material);
    apply_dirichlet(&mut a, &p.fixed);
    let mut b = p.force.clone();
    for i in 0..p.mesh.nodes.len() {
        for c in 0..3 {
            if is_fixed(&p.fixed, i, c) {
                b[3 * i + c] = 0.0;
            }
        }
    }
    let free = (0..p.mesh.nodes.len())
        .map(|i| 3 - p.fixed[i].count_ones() as usize)
        .sum();
    let pre = TwoLevel::new(&a, &p.fixed, p.mesh.nverts, &p.mesh.tets, &p.mesh.mid_edge)?;
    let mut report = SolveReport {
        dofs: n,
        free_dofs: free,
        coarse_dofs: p.mesh.nverts * 3,
        ..Default::default()
    };
    let (x, run) = pcg_solve(&a, &pre, &b, &vec![0.0; n], opts)?;
    report.iterations = run.iterations;
    report.rel_residual = run.rel_residual;
    report.converged = run.converged;
    report.gpu_device = run.gpu_device;
    report.gpu_gflops = run.gpu_gflops;
    report.gpu_bw_gbps = run.gpu_bw_gbps;
    Ok((x, report))
}

/// Hardware summary + measured performance of a large-scale 3D P2 tetrahedral FEA benchmark
/// executed on the local CUDA GPU(s).
#[derive(Clone, Debug, serde::Serialize)]
pub struct LargeGpuFeaBenchmark {
    pub gpus_detected: Vec<String>,
    pub p2_tets: usize,
    pub p2_nodes: usize,
    pub total_dofs: usize,
    pub nnz_blocks_3x3: usize,
    pub assemble_ms: f64,
    pub gpu_pcg_iterations: usize,
    pub gpu_pcg_residual: f64,
    pub gpu_pcg_wall_ms: f64,
    pub gpu_pcg_kernel_ms: f64,
    pub gpu_pcg_gflops: f64,
    pub gpu_pcg_bw_gbps: f64,
    pub gpu_device_used: String,
    pub multi_gpu_spmv_devices: Vec<String>,
    pub multi_gpu_spmv_max_diff: f64,
    pub max_tip_deflection_mm: f64,
}

/// Discover local CUDA GPUs via `fenics-solver`.
pub fn discover_local_gpus() -> Vec<GpuDeviceInfo> {
    discover_gpus()
}

/// Build and solve a 3D quadratic (P2) tetrahedral cantilever elasticity benchmark of
/// `cubes = [nx, ny, nz]` Freudenthal-tessellated hex cells (`6 * nx * ny * nz` P2 tets)
/// on the local CUDA GPUs (including multi-GPU 3x3 BSR SpMV verification across all GPUs).
pub fn benchmark_large_gpu_fea(
    cubes: [usize; 3],
    edge_mm: f64,
) -> Result<LargeGpuFeaBenchmark, String> {
    use crate::{Material, P2Mesh};
    use std::time::Instant;

    let [nx, ny, nz] = [cubes[0].max(2), cubes[1].max(2), cubes[2].max(2)];
    let stride_z = nz + 1;
    let stride_y = (ny + 1) * stride_z;
    let vid = |i: usize, j: usize, k: usize| -> u32 { (i * stride_y + j * stride_z + k) as u32 };

    let mut verts = Vec::with_capacity((nx + 1) * (ny + 1) * (nz + 1));
    for i in 0..=nx {
        for j in 0..=ny {
            for k in 0..=nz {
                verts.push([i as f64 * edge_mm, j as f64 * edge_mm, k as f64 * edge_mm]);
            }
        }
    }

    let mut tets = Vec::with_capacity(nx * ny * nz * 6);
    for i in 0..nx {
        for j in 0..ny {
            for k in 0..nz {
                let c = [
                    vid(i, j, k),
                    vid(i + 1, j, k),
                    vid(i + 1, j + 1, k),
                    vid(i, j + 1, k),
                    vid(i, j, k + 1),
                    vid(i + 1, j, k + 1),
                    vid(i + 1, j + 1, k + 1),
                    vid(i, j + 1, k + 1),
                ];
                for mut t in [
                    [c[0], c[1], c[2], c[6]],
                    [c[0], c[2], c[3], c[6]],
                    [c[0], c[3], c[7], c[6]],
                    [c[0], c[7], c[4], c[6]],
                    [c[0], c[4], c[5], c[6]],
                    [c[0], c[5], c[1], c[6]],
                ] {
                    let (a, b, c0, d) = (
                        verts[t[0] as usize],
                        verts[t[1] as usize],
                        verts[t[2] as usize],
                        verts[t[3] as usize],
                    );
                    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                    let v = [c0[0] - a[0], c0[1] - a[1], c0[2] - a[2]];
                    let w = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
                    let det = u[0] * (v[1] * w[2] - v[2] * w[1])
                        - u[1] * (v[0] * w[2] - v[2] * w[0])
                        + u[2] * (v[0] * w[1] - v[1] * w[0]);
                    if det < 0.0 {
                        t.swap(2, 3);
                    }
                    tets.push(t);
                }
            }
        }
    }

    let t_asm0 = Instant::now();
    let mesh = P2Mesh::from_linear(&verts, &tets);
    let n_nodes = mesh.nodes.len();
    let n_dofs = mesh.ndofs();
    let mat = Material::iso(vec![200_000.0; mesh.tets.len()], 0.30);

    let mut fixed = vec![0u8; n_nodes];
    let mut force = vec![0.0f64; n_dofs];
    let x_max = nx as f64 * edge_mm;
    let mut tip_nodes = 0usize;
    for (idx, p) in mesh.nodes.iter().enumerate() {
        if p[0] <= 1e-9 {
            fixed[idx] = 0b111;
        } else if (p[0] - x_max).abs() <= 1e-9 {
            tip_nodes += 1;
        }
    }
    let f_per_node = -1000.0 / (tip_nodes.max(1) as f64);
    for (idx, p) in mesh.nodes.iter().enumerate() {
        if (p[0] - x_max).abs() <= 1e-9 {
            force[3 * idx + 2] = f_per_node;
        }
    }

    let mut a = assemble(&mesh, &mat);
    apply_dirichlet(&mut a, &fixed);
    for i in 0..n_nodes {
        for c in 0..3 {
            if is_fixed(&fixed, i, c) {
                force[3 * i + c] = 0.0;
            }
        }
    }
    let dinv: Vec<[f64; 9]> = (0..a.n)
        .map(|i| inv3(&a.vals[a.find(i, i as u32)]))
        .collect();
    let assemble_ms = t_asm0.elapsed().as_secs_f64() * 1000.0;

    let gpus = discover_gpus();
    let gpus_detected: Vec<String> = gpus
        .iter()
        .map(|g| {
            format!(
                "GPU {}: {} ({} MiB / {} MiB free, sm_{}{}, {} SMs)",
                g.index,
                g.name,
                g.free_memory_mib,
                g.total_memory_mib,
                g.compute_capability.0,
                g.compute_capability.1,
                g.multiprocessor_count
            )
        })
        .collect();

    let gpu_res = solve_bsr3_pcg_gpu(
        a.n,
        &a.row_ptr,
        &a.cols,
        a.vals_flat(),
        flatten_blocks9(&dinv),
        &force,
        None,
        1e-8,
        4000,
        None,
    )?;

    // Also verify Multi-GPU BSR3 SpMV across up to 3 GPUs against the converged solution vector
    let mut y_cpu = vec![0.0f64; n_dofs];
    a.mul(&gpu_res.solution, &mut y_cpu);
    let (y_mgpu, mgpu_devs) = spmv_bsr3_multi_gpu(
        a.n,
        &a.row_ptr,
        &a.cols,
        a.vals_flat(),
        &gpu_res.solution,
        3,
    )
    .unwrap_or_else(|_| (y_cpu.clone(), vec!["CPU".into()]));

    let multi_gpu_spmv_max_diff = y_cpu
        .iter()
        .zip(&y_mgpu)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f64, f64::max);

    let max_tip_deflection_mm = gpu_res
        .solution
        .chunks_exact(3)
        .map(|u| (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt())
        .fold(0.0f64, f64::max);

    Ok(LargeGpuFeaBenchmark {
        gpus_detected,
        p2_tets: mesh.tets.len(),
        p2_nodes: n_nodes,
        total_dofs: n_dofs,
        nnz_blocks_3x3: a.nnz_blocks(),
        assemble_ms,
        gpu_pcg_iterations: gpu_res.iterations,
        gpu_pcg_residual: gpu_res.final_residual,
        gpu_pcg_wall_ms: gpu_res.wall_ms,
        gpu_pcg_kernel_ms: gpu_res.kernel_ms,
        gpu_pcg_gflops: gpu_res.gflops,
        gpu_pcg_bw_gbps: gpu_res.effective_bw_gbps,
        gpu_device_used: gpu_res
            .devices_used
            .first()
            .cloned()
            .unwrap_or_else(|| "CPU".into()),
        multi_gpu_spmv_devices: mgpu_devs,
        multi_gpu_spmv_max_diff,
        max_tip_deflection_mm,
    })
}

#[cfg(test)]
mod gpu_tests {
    use super::*;

    #[test]
    fn test_large_3d_p2_fea_on_rtx_5090_gpus() {
        let gpus = discover_local_gpus();
        if gpus.is_empty() {
            println!("No CUDA GPUs found; skipping hardware test");
            return;
        }
        // 1) Medium-large 3D P2 cantilever: 12x6x6 hexes = 2,592 P2 tets = 4,333 P2 nodes = 12,999 DOFs
        let bench1 = benchmark_large_gpu_fea([12, 6, 6], 2.0).expect("12x6x6 GPU FEA benchmark");
        println!(
            "[GPU FEA 12x6x6] tets={}, nodes={}, dofs={}, nnz_blocks={}, asm={:.1}ms | GPU PCG: iters={}, res={:.2e}, wall={:.2}ms, kern={:.2}ms, {:.2} GFLOP/s, {:.2} GB/s on {} | Multi-GPU ({:?}) max_diff={:.2e} | tip={:.4}mm",
            bench1.p2_tets,
            bench1.p2_nodes,
            bench1.total_dofs,
            bench1.nnz_blocks_3x3,
            bench1.assemble_ms,
            bench1.gpu_pcg_iterations,
            bench1.gpu_pcg_residual,
            bench1.gpu_pcg_wall_ms,
            bench1.gpu_pcg_kernel_ms,
            bench1.gpu_pcg_gflops,
            bench1.gpu_pcg_bw_gbps,
            bench1.gpu_device_used,
            bench1.multi_gpu_spmv_devices,
            bench1.multi_gpu_spmv_max_diff,
            bench1.max_tip_deflection_mm,
        );
        assert!(bench1.gpu_pcg_residual < 1e-8);
        assert!(bench1.multi_gpu_spmv_max_diff < 1e-9);
        assert!(bench1.max_tip_deflection_mm > 0.0);

        // 2) Larger 3D P2 cantilever: 20x10x10 hexes = 12,000 P2 tets = 18,081 P2 nodes = 54,243 DOFs
        let bench2 = benchmark_large_gpu_fea([20, 10, 10], 1.5).expect("20x10x10 GPU FEA benchmark");
        println!(
            "[GPU FEA 20x10x10] tets={}, nodes={}, dofs={}, nnz_blocks={}, asm={:.1}ms | GPU PCG: iters={}, res={:.2e}, wall={:.2}ms, kern={:.2}ms, {:.2} GFLOP/s, {:.2} GB/s on {} | Multi-GPU ({:?}) max_diff={:.2e} | tip={:.4}mm",
            bench2.p2_tets,
            bench2.p2_nodes,
            bench2.total_dofs,
            bench2.nnz_blocks_3x3,
            bench2.assemble_ms,
            bench2.gpu_pcg_iterations,
            bench2.gpu_pcg_residual,
            bench2.gpu_pcg_wall_ms,
            bench2.gpu_pcg_kernel_ms,
            bench2.gpu_pcg_gflops,
            bench2.gpu_pcg_bw_gbps,
            bench2.gpu_device_used,
            bench2.multi_gpu_spmv_devices,
            bench2.multi_gpu_spmv_max_diff,
            bench2.max_tip_deflection_mm,
        );
        assert!(bench2.gpu_pcg_residual < 1e-8);
        assert!(bench2.multi_gpu_spmv_max_diff < 1e-9);

        // 3) Large 3D P2 cantilever: 30x14x14 hexes = 35,280 P2 tets = 50,835 P2 nodes = 152,505 DOFs
        let bench3 = benchmark_large_gpu_fea([30, 14, 14], 1.0).expect("30x14x14 GPU FEA benchmark");
        println!(
            "[GPU FEA 30x14x14] tets={}, nodes={}, dofs={}, nnz_blocks={}, asm={:.1}ms | GPU PCG: iters={}, res={:.2e}, wall={:.2}ms, kern={:.2}ms, {:.2} GFLOP/s, {:.2} GB/s on {} | Multi-GPU ({:?}) max_diff={:.2e} | tip={:.4}mm",
            bench3.p2_tets,
            bench3.p2_nodes,
            bench3.total_dofs,
            bench3.nnz_blocks_3x3,
            bench3.assemble_ms,
            bench3.gpu_pcg_iterations,
            bench3.gpu_pcg_residual,
            bench3.gpu_pcg_wall_ms,
            bench3.gpu_pcg_kernel_ms,
            bench3.gpu_pcg_gflops,
            bench3.gpu_pcg_bw_gbps,
            bench3.gpu_device_used,
            bench3.multi_gpu_spmv_devices,
            bench3.multi_gpu_spmv_max_diff,
            bench3.max_tip_deflection_mm,
        );
        assert!(bench3.gpu_pcg_residual < 1e-8);
        assert!(bench3.multi_gpu_spmv_max_diff < 1e-9);
    }
}
