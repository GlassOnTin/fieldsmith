//! Small fixed-size vector helpers and the dual number type `D = (value, gradient)`.

pub type V3 = [f64; 3];

#[inline]
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
pub fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
pub fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
#[inline]
pub fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
#[inline]
pub fn normalize(a: V3) -> V3 {
    let l = len(a);
    if l > 1e-12 {
        scale(a, 1.0 / l)
    } else {
        [0.0, 0.0, 0.0]
    }
}

/// Row-major 3×3 matrix.
pub type M3 = [[f64; 3]; 3];

#[inline]
pub fn mat_vec(m: &M3, v: V3) -> V3 {
    [dot(m[0], v), dot(m[1], v), dot(m[2], v)]
}
#[inline]
pub fn mat_t_vec(m: &M3, v: V3) -> V3 {
    [
        m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2],
        m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2],
        m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2],
    ]
}
pub fn mat_mul(a: &M3, b: &M3) -> M3 {
    let mut o = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            o[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    o
}
pub const IDENTITY: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// Rotation `R = Rz · Ry · Rx` from XYZ angles in degrees.
pub fn rotation_xyz_deg(r: V3) -> M3 {
    let (sx, cx) = r[0].to_radians().sin_cos();
    let (sy, cy) = r[1].to_radians().sin_cos();
    let (sz, cz) = r[2].to_radians().sin_cos();
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    mat_mul(&rz, &mat_mul(&ry, &rx))
}

/// Rotation by `deg` about a principal axis.
pub fn rotation_axis_deg(axis: usize, deg: f64) -> M3 {
    let mut r = [0.0; 3];
    r[axis] = deg;
    rotation_xyz_deg(r)
}

/// A field sample: signed distance estimate and its spatial gradient.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct D {
    pub v: f64,
    pub g: V3,
}

impl D {
    pub const EMPTY: D = D {
        v: 1e9,
        g: [0.0, 0.0, 0.0],
    };
    #[inline]
    pub fn new(v: f64, g: V3) -> Self {
        Self { v, g }
    }
}

impl std::ops::Neg for D {
    type Output = D;
    #[inline]
    fn neg(self) -> Self {
        D {
            v: -self.v,
            g: scale(self.g, -1.0),
        }
    }
}

#[inline]
pub fn dmin(a: D, b: D) -> D {
    if a.v <= b.v {
        a
    } else {
        b
    }
}

#[inline]
pub fn dmax(a: D, b: D) -> D {
    if a.v >= b.v {
        a
    } else {
        b
    }
}

/// Effective fillet radius for a blend of nominal radius `r` between operands `a` and `b`.
///
/// The radius is scaled by `w = clamp(1 − n̂a·n̂b, 0, 1)` so that surfaces meeting edge-on
/// (parallel normals, e.g. coplanar seams) stay sharp and only genuine corners receive a
/// fillet; at 90° `k = r`. GLSL mirror: `fs_blendk`.
#[inline]
pub fn blend_radius(a: D, b: D, r: f64) -> f64 {
    if r <= 0.0 {
        return 0.0;
    }
    r * (1.0 - dot(normalize(a.g), normalize(b.g))).clamp(0.0, 1.0)
}

/// Rolling-ball contact points for a blend of radius `k` at `p`.
///
/// The round union treats both operands locally as planes; the ball centre is then
/// `c = p + (k − x)n̂x + (k − y)n̂y` and it touches the operands at `t_x = c − k n̂x` and
/// `t_y = c − k n̂y`. Returns `(t_x, t_y, cosθ)`. GLSL mirror: inlined in combine functions.
#[inline]
pub fn blend_contacts(p: V3, x: D, y: D, k: f64) -> (V3, V3, f64) {
    let nx = normalize(x.g);
    let ny = normalize(y.g);
    let c = add(add(p, scale(nx, k - x.v)), scale(ny, k - y.v));
    (sub(c, scale(nx, k)), sub(c, scale(ny, k)), dot(nx, ny))
}

/// Contact gate: for genuine planar faces the operand values at the contact points are exactly
/// `x(t_x) = (k − y)·cosθ` and `y(t_y) = (k − x)·cosθ`. Where an operand's face ends (the end of
/// a fillet run, or near a flush seam where the nearest feature is an edge) the contact lands off
/// the surface and the residual grows; the fillet fades out over a residual of `k/4`. This keeps
/// blends from bulging past end faces and along coplanar joints. GLSL mirror: `fs_cgate`.
#[inline]
pub fn blend_gate(x_at_tx: f64, y_at_ty: f64, x: f64, y: f64, k: f64, cos: f64) -> f64 {
    let res = (x_at_tx - (k - y) * cos).abs() + (y_at_ty - (k - x) * cos).abs();
    (1.0 - res / (0.25 * k)).clamp(0.0, 1.0)
}

/// Circular ("rolling ball") round union with fillet radius `k`: where both operands are
/// within `k` of their surfaces the result is `k − |(k − a, k − b)|`, which for perpendicular
/// planes is the exact distance to a fillet of radius `k`, inside and outside.
/// GLSL mirror: `fs_rmin`.
#[inline]
pub fn round_min(a: D, b: D, k: f64) -> D {
    if k <= 1e-9 {
        return dmin(a, b);
    }
    let u = (k - a.v).max(0.0);
    let v = (k - b.v).max(0.0);
    if u <= 0.0 || v <= 0.0 {
        return dmin(a, b);
    }
    let l = (u * u + v * v).sqrt();
    D {
        v: k - l,
        g: scale(add(scale(a.g, u), scale(b.g, v)), 1.0 / l),
    }
}
