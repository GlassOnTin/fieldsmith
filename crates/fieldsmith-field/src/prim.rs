//! Primitive signed distance functions with analytic gradients, in each primitive's local frame.
//! Every function here has a line-for-line GLSL twin in [`crate::glsl::PRELUDE`].

use crate::dual::*;
use fieldsmith_doc::Face;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcSeg {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub bulge: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Box {
        half: V3,
        round: f64,
    },
    Cylinder {
        radius: f64,
        half_h: f64,
        round: f64,
    },
    Sphere {
        radius: f64,
    },
    Capsule {
        radius: f64,
        half_len: f64,
    },
    Torus {
        major: f64,
        minor: f64,
    },
    Extrude {
        segs: Vec<ArcSeg>,
        half_h: f64,
    },
}

#[inline]
fn sgn(x: f64) -> f64 {
    if x < 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// Combine a 2D signed distance `w.0` (with gradient `gw0` in xy) and an axial distance
/// `w.1 = |z| − h` into a 3D extrusion distance.
#[inline]
fn extrude_combine(wx: f64, gx: [f64; 2], wy: f64, sz: f64) -> D {
    let ox = wx.max(0.0);
    let oy = wy.max(0.0);
    let ol = (ox * ox + oy * oy).sqrt();
    if ol > 0.0 {
        let cx = ox / ol;
        let cy = oy / ol;
        D::new(ol, [cx * gx[0], cx * gx[1], cy * sz])
    } else if wx > wy {
        D::new(wx, [gx[0], gx[1], 0.0])
    } else {
        D::new(wy, [0.0, 0.0, sz])
    }
}

pub fn eval_prim(prim: &Prim, p: V3) -> D {
    match prim {
        Prim::Sphere { radius } => {
            let l = len(p);
            let g = if l > 1e-12 {
                scale(p, 1.0 / l)
            } else {
                [0.0, 0.0, 1.0]
            };
            D::new(l - radius, g)
        }
        Prim::Box { half, round } => {
            let q = [
                p[0].abs() - (half[0] - round),
                p[1].abs() - (half[1] - round),
                p[2].abs() - (half[2] - round),
            ];
            let o = [q[0].max(0.0), q[1].max(0.0), q[2].max(0.0)];
            let ol = len(o);
            if ol > 0.0 {
                let g = [
                    sgn(p[0]) * o[0] / ol,
                    sgn(p[1]) * o[1] / ol,
                    sgn(p[2]) * o[2] / ol,
                ];
                D::new(ol - round, g)
            } else {
                let mut i = 0;
                if q[1] > q[i] {
                    i = 1;
                }
                if q[2] > q[i] {
                    i = 2;
                }
                let mut g = [0.0; 3];
                g[i] = sgn(p[i]);
                D::new(q[i] - round, g)
            }
        }
        Prim::Cylinder {
            radius,
            half_h,
            round,
        } => {
            let rl = (p[0] * p[0] + p[1] * p[1]).sqrt();
            let u = if rl > 1e-12 {
                [p[0] / rl, p[1] / rl]
            } else {
                [1.0, 0.0]
            };
            let d = extrude_combine(
                rl - (radius - round),
                u,
                p[2].abs() - (half_h - round),
                sgn(p[2]),
            );
            D::new(d.v - round, d.g)
        }
        Prim::Capsule { radius, half_len } => {
            let q = [p[0], p[1], p[2] - p[2].clamp(-half_len, *half_len)];
            let l = len(q);
            let g = if l > 1e-12 {
                scale(q, 1.0 / l)
            } else {
                [1.0, 0.0, 0.0]
            };
            D::new(l - radius, g)
        }
        Prim::Torus { major, minor } => {
            let rl = (p[0] * p[0] + p[1] * p[1]).sqrt();
            let u = if rl > 1e-12 {
                [p[0] / rl, p[1] / rl]
            } else {
                [1.0, 0.0]
            };
            let qx = rl - major;
            let qz = p[2];
            let l = (qx * qx + qz * qz).sqrt();
            let (cx, cz) = if l > 1e-12 {
                (qx / l, qz / l)
            } else {
                (1.0, 0.0)
            };
            D::new(l - minor, [cx * u[0], cx * u[1], cz])
        }
        Prim::Extrude { segs, half_h } => {
            let (s, g2) = profile_sd(segs, [p[0], p[1]]);
            extrude_combine(s, g2, p[2].abs() - half_h, sgn(p[2]))
        }
    }
}

/// Arc geometry: (centre, radius, arc midpoint, cos(half angle)).
pub fn arc_geom(s: &ArcSeg) -> ([f64; 2], f64, [f64; 2], f64) {
    let (a, b, k) = (s.a, s.b, s.bulge);
    let c = [b[0] - a[0], b[1] - a[1]];
    let l = (c[0] * c[0] + c[1] * c[1]).sqrt();
    let d = [c[0] / l, c[1] / l];
    let nl = [-d[1], d[0]];
    let m = [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])];
    let h = 0.5 * l * (1.0 - k * k) / (2.0 * k);
    let centre = [m[0] + nl[0] * h, m[1] + nl[1] * h];
    let radius = 0.5 * l * (1.0 + k * k) / (2.0 * k.abs());
    let sag = k * l * 0.5;
    let mid = [m[0] - nl[0] * sag, m[1] - nl[1] * sag];
    let half_angle = 2.0 * k.abs().atan();
    (centre, radius, mid, half_angle.cos())
}

/// Exact signed distance to a closed profile of line and arc segments, with its gradient.
pub fn profile_sd(segs: &[ArcSeg], p: [f64; 2]) -> (f64, [f64; 2]) {
    let mut best = f64::INFINITY;
    let mut best_vec = [1.0, 0.0];
    let mut inside = false;
    for s in segs {
        let (a, b) = (s.a, s.b);
        let e = [b[0] - a[0], b[1] - a[1]];
        // Crossing number for the chord.
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * e[0];
            if p[0] < x {
                inside = !inside;
            }
        }
        let closest;
        if s.bulge == 0.0 {
            let ee = e[0] * e[0] + e[1] * e[1];
            let t = if ee > 0.0 {
                (((p[0] - a[0]) * e[0] + (p[1] - a[1]) * e[1]) / ee).clamp(0.0, 1.0)
            } else {
                0.0
            };
            closest = [a[0] + e[0] * t, a[1] + e[1] * t];
        } else {
            let (c, r, mid, cos_half) = arc_geom(s);
            let v = [p[0] - c[0], p[1] - c[1]];
            let vl = (v[0] * v[0] + v[1] * v[1]).sqrt();
            let md = [(mid[0] - c[0]) / r, (mid[1] - c[1]) / r];
            let within = vl > 1e-12 && (v[0] * md[0] + v[1] * md[1]) / vl >= cos_half;
            if within {
                closest = [c[0] + v[0] / vl * r, c[1] + v[1] / vl * r];
            } else {
                let da = (p[0] - a[0]).powi(2) + (p[1] - a[1]).powi(2);
                let db = (p[0] - b[0]).powi(2) + (p[1] - b[1]).powi(2);
                closest = if da <= db { a } else { b };
            }
            // Circular segment between chord and arc toggles insideness.
            let side = e[0] * (p[1] - a[1]) - e[1] * (p[0] - a[0]);
            if vl < r && side * s.bulge < 0.0 {
                inside = !inside;
            }
        }
        let dv = [p[0] - closest[0], p[1] - closest[1]];
        let dl = (dv[0] * dv[0] + dv[1] * dv[1]).sqrt();
        if dl < best {
            best = dl;
            best_vec = if dl > 1e-12 {
                [dv[0] / dl, dv[1] / dl]
            } else {
                // On the boundary: use the segment's right-hand normal (outward for CCW profiles).
                let el = (e[0] * e[0] + e[1] * e[1]).sqrt().max(1e-12);
                [e[1] / el, -e[0] / el]
            };
        }
    }
    if inside {
        (-best, [-best_vec[0], -best_vec[1]])
    } else {
        (best, best_vec)
    }
}

/// Which face of the primitive the local point `p` is closest to.
pub fn classify_face(prim: &Prim, p: V3) -> Face {
    match prim {
        Prim::Box { half, .. } => {
            let q = [
                p[0].abs() - half[0],
                p[1].abs() - half[1],
                p[2].abs() - half[2],
            ];
            let mut i = 0;
            if q[1] > q[i] {
                i = 1;
            }
            if q[2] > q[i] {
                i = 2;
            }
            match (i, p[i] >= 0.0) {
                (0, true) => Face::Px,
                (0, false) => Face::Nx,
                (1, true) => Face::Py,
                (1, false) => Face::Ny,
                (2, true) => Face::Pz,
                _ => Face::Nz,
            }
        }
        Prim::Cylinder { radius, half_h, .. } => {
            let side = (p[0] * p[0] + p[1] * p[1]).sqrt() - radius;
            let cap = p[2].abs() - half_h;
            if side >= cap {
                Face::Side
            } else if p[2] >= 0.0 {
                Face::Top
            } else {
                Face::Bottom
            }
        }
        Prim::Extrude { segs, half_h } => {
            let (s, _) = profile_sd(segs, [p[0], p[1]]);
            let cap = p[2].abs() - half_h;
            if s >= cap {
                Face::Side
            } else if p[2] >= 0.0 {
                Face::Top
            } else {
                Face::Bottom
            }
        }
        _ => Face::All,
    }
}

/// Faces a primitive exposes (for UI pickers).
pub fn faces_of(prim: &Prim) -> &'static [Face] {
    match prim {
        Prim::Box { .. } => &[Face::Px, Face::Nx, Face::Py, Face::Ny, Face::Pz, Face::Nz],
        Prim::Cylinder { .. } | Prim::Extrude { .. } => &[Face::Side, Face::Top, Face::Bottom],
        _ => &[],
    }
}

/// Conservative local-frame bounding box.
pub fn prim_bounds(prim: &Prim) -> (V3, V3) {
    match prim {
        Prim::Box { half, .. } => ([-half[0], -half[1], -half[2]], *half),
        Prim::Cylinder { radius, half_h, .. } => {
            ([-radius, -radius, -half_h], [*radius, *radius, *half_h])
        }
        Prim::Sphere { radius } => ([-radius; 3], [*radius; 3]),
        Prim::Capsule { radius, half_len } => (
            [-radius, -radius, -half_len - radius],
            [*radius, *radius, half_len + radius],
        ),
        Prim::Torus { major, minor } => {
            let r = major + minor;
            ([-r, -r, -minor], [r, r, *minor])
        }
        Prim::Extrude { segs, half_h } => {
            let mut lo = [f64::INFINITY; 2];
            let mut hi = [f64::NEG_INFINITY; 2];
            for s in segs {
                let mut pts = vec![s.a, s.b];
                if s.bulge != 0.0 {
                    let (c, r, _, _) = arc_geom(s);
                    pts.push([c[0] - r, c[1] - r]);
                    pts.push([c[0] + r, c[1] + r]);
                }
                for q in pts {
                    for i in 0..2 {
                        lo[i] = lo[i].min(q[i]);
                        hi[i] = hi[i].max(q[i]);
                    }
                }
            }
            ([lo[0], lo[1], -half_h], [hi[0], hi[1], *half_h])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fd_grad(prim: &Prim, p: V3) -> V3 {
        let h = 1e-6;
        let mut g = [0.0; 3];
        for i in 0..3 {
            let mut a = p;
            let mut b = p;
            a[i] += h;
            b[i] -= h;
            g[i] = (eval_prim(prim, a).v - eval_prim(prim, b).v) / (2.0 * h);
        }
        g
    }

    #[test]
    fn analytic_gradients_match_finite_differences() {
        let square_with_arc = vec![
            ArcSeg {
                a: [0.0, 0.0],
                b: [10.0, 0.0],
                bulge: 0.0,
            },
            ArcSeg {
                a: [10.0, 0.0],
                b: [10.0, 10.0],
                bulge: 0.4,
            },
            ArcSeg {
                a: [10.0, 10.0],
                b: [0.0, 10.0],
                bulge: 0.0,
            },
            ArcSeg {
                a: [0.0, 10.0],
                b: [0.0, 0.0],
                bulge: -0.3,
            },
        ];
        let prims = vec![
            Prim::Sphere { radius: 3.0 },
            Prim::Box {
                half: [3.0, 2.0, 1.0],
                round: 0.0,
            },
            Prim::Box {
                half: [3.0, 2.0, 1.5],
                round: 0.5,
            },
            Prim::Cylinder {
                radius: 2.0,
                half_h: 3.0,
                round: 0.0,
            },
            Prim::Cylinder {
                radius: 2.0,
                half_h: 3.0,
                round: 0.4,
            },
            Prim::Capsule {
                radius: 1.0,
                half_len: 2.0,
            },
            Prim::Torus {
                major: 3.0,
                minor: 1.0,
            },
            Prim::Extrude {
                segs: square_with_arc,
                half_h: 2.0,
            },
        ];
        let pts = [
            [0.3, 0.7, 4.1],
            [4.2, -0.4, 0.2],
            [1.1, 0.2, -0.3],
            [-2.7, 1.83, 0.91],
            [5.5, 5.2, 1.0],
            [11.5, 5.0, 0.5],
            [-0.8, 5.0, 0.1],
        ];
        for prim in &prims {
            for &p in &pts {
                let d = eval_prim(prim, p);
                let g = fd_grad(prim, p);
                for i in 0..3 {
                    assert!(
                        (d.g[i] - g[i]).abs() < 1e-4,
                        "{prim:?} at {p:?}: analytic {:?} vs fd {:?}",
                        d.g,
                        g
                    );
                }
            }
        }
    }

    #[test]
    fn profile_with_arc_is_exact() {
        // Unit-radius semicircle bulging out of a 2×2 square's right side: (0,0)-(2,0)-arc-(2,2)-(0,2).
        let segs = vec![
            ArcSeg {
                a: [0.0, 0.0],
                b: [2.0, 0.0],
                bulge: 0.0,
            },
            ArcSeg {
                a: [2.0, 0.0],
                b: [2.0, 2.0],
                bulge: 1.0,
            },
            ArcSeg {
                a: [2.0, 2.0],
                b: [0.0, 2.0],
                bulge: 0.0,
            },
            ArcSeg {
                a: [0.0, 2.0],
                b: [0.0, 0.0],
                bulge: 0.0,
            },
        ];
        // Point on the arc's far side: arc reaches x = 3 at y = 1.
        let (d, _) = profile_sd(&segs, [3.5, 1.0]);
        assert!((d - 0.5).abs() < 1e-12);
        let (d, _) = profile_sd(&segs, [2.5, 1.0]);
        assert!((d + 0.5).abs() < 1e-12, "inside the bulge: {d}");
        let (d, _) = profile_sd(&segs, [1.0, 1.0]);
        assert!((d + 1.0).abs() < 1e-12);
    }
}
