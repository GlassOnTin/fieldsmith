//! Point queries on an analysis: which element contains a point, and the interpolated result.

use crate::Analysis;
use fieldsmith_element_shim::basis;
use serde::Serialize;

mod fieldsmith_element_shim {
    use fenics_element::{FiniteElement, P2Tetrahedron};
    pub fn basis(xi: [f64; 3]) -> Vec<f64> {
        P2Tetrahedron.evaluate_basis(&xi)
    }
}

/// Uniform grid of element bounding boxes for point location.
pub struct Locator {
    lo: [f64; 3],
    cell: f64,
    n: [usize; 3],
    bins: Vec<Vec<u32>>,
}

impl Locator {
    pub fn new(a: &Analysis) -> Locator {
        let m = &a.mesh;
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in &m.nodes[..m.nverts] {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let cell = (2.0 * a.provenance.element_size).max(1e-6);
        let n = [0, 1, 2].map(|k| (((hi[k] - lo[k]) / cell).ceil() as usize).max(1));
        let mut bins = vec![Vec::new(); n[0] * n[1] * n[2]];
        for (e, t) in m.tets.iter().enumerate() {
            let mut tl = [f64::INFINITY; 3];
            let mut th = [f64::NEG_INFINITY; 3];
            for &v in &t[..4] {
                let p = m.nodes[v as usize];
                for k in 0..3 {
                    tl[k] = tl[k].min(p[k]);
                    th[k] = th[k].max(p[k]);
                }
            }
            let i0 = [0, 1, 2].map(|k| (((tl[k] - lo[k]) / cell).floor().max(0.0) as usize).min(n[k] - 1));
            let i1 = [0, 1, 2].map(|k| (((th[k] - lo[k]) / cell).floor().max(0.0) as usize).min(n[k] - 1));
            for z in i0[2]..=i1[2] {
                for y in i0[1]..=i1[1] {
                    for x in i0[0]..=i1[0] {
                        bins[x + n[0] * (y + n[1] * z)].push(e as u32);
                    }
                }
            }
        }
        Locator { lo, cell, n, bins }
    }

    /// Element containing `p` and its barycentric coordinates. Points just outside the mesh
    /// (within half an element, e.g. on a curved face between chords) snap to the nearest element.
    pub fn locate(&self, a: &Analysis, p: [f64; 3]) -> Option<(usize, [f64; 4])> {
        let idx = [0, 1, 2].map(|k| ((p[k] - self.lo[k]) / self.cell).floor() as i64);
        let mut best: Option<(usize, [f64; 4], f64)> = None;
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let c = [idx[0] + dx, idx[1] + dy, idx[2] + dz];
                    if (0..3).any(|k| c[k] < 0 || c[k] >= self.n[k] as i64) {
                        continue;
                    }
                    let bin = &self.bins[c[0] as usize + self.n[0] * (c[1] as usize + self.n[1] * c[2] as usize)];
                    for &e in bin {
                        let l = barycentric(a, e as usize, p);
                        let worst = l.iter().cloned().fold(f64::INFINITY, f64::min);
                        if worst >= -1e-9 {
                            return Some((e as usize, l));
                        }
                        if best.as_ref().is_none_or(|b| worst > b.2) {
                            best = Some((e as usize, l, worst));
                        }
                    }
                }
            }
        }
        let (e, l, worst) = best?;
        if worst < -0.5 {
            return None;
        }
        let mut c = l.map(|x| x.max(0.0));
        let s: f64 = c.iter().sum();
        c.iter_mut().for_each(|x| *x /= s);
        Some((e, c))
    }
}

fn barycentric(a: &Analysis, e: usize, p: [f64; 3]) -> [f64; 4] {
    let t = &a.mesh.tets[e];
    let v = [0, 1, 2, 3].map(|i| a.mesh.nodes[t[i] as usize]);
    let d = |x: [f64; 3], y: [f64; 3]| [x[0] - y[0], x[1] - y[1], x[2] - y[2]];
    let (e1, e2, e3, r) = (d(v[1], v[0]), d(v[2], v[0]), d(v[3], v[0]), d(p, v[0]));
    let det = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]);
    let dt = det(e1, e2, e3);
    let l1 = det(r, e2, e3) / dt;
    let l2 = det(e1, r, e3) / dt;
    let l3 = det(e1, e2, r) / dt;
    [1.0 - l1 - l2 - l3, l1, l2, l3]
}

#[derive(Clone, Debug, Serialize)]
pub struct ProbeValue {
    pub point: [f64; 3],
    pub element: usize,
    pub von_mises: f64,
    pub stress: [f64; 6],
    pub displacement: [f64; 3],
    pub safety_factor: f64,
}

/// Interpolate case `case` (or the envelope von Mises when `None`) at `p` with the P2 basis.
pub fn probe(a: &Analysis, loc: &Locator, case: Option<usize>, p: [f64; 3]) -> Option<ProbeValue> {
    let (e, l) = loc.locate(a, p)?;
    let phi = basis([l[1], l[2], l[3]]);
    let t = &a.mesh.tets[e];
    let mut s = [0.0; 6];
    let mut u = [0.0; 3];
    let mut vm = 0.0;
    for (k, w) in phi.iter().enumerate() {
        let n = t[k] as usize;
        match case {
            Some(c) => {
                let r = &a.cases[c];
                for (j, sj) in s.iter_mut().enumerate() {
                    *sj += w * r.stress[n][j];
                }
                for (j, uj) in u.iter_mut().enumerate() {
                    *uj += w * r.u[3 * n + j];
                }
            }
            None => vm += w * a.envelope[n],
        }
    }
    if case.is_some() {
        vm = fieldsmith_solve::von_mises(&s);
    }
    Some(ProbeValue {
        point: p,
        element: e,
        von_mises: vm,
        stress: s,
        displacement: u,
        safety_factor: if vm > 0.0 { a.yield_strength / vm } else { f64::INFINITY },
    })
}
