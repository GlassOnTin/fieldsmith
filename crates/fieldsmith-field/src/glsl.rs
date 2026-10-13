//! GLSL ES 3.00 code generation. The generated `vec4 fs_field(vec3 p)` returns
//! `(distance, gradient)` and mirrors [`crate::expr::eval`] formula for formula.

use crate::dual::*;
use crate::expr::placed_shape;
use crate::expr::Expr;
use crate::lattice::FillExpr;
use crate::prim::{arc_geom, Prim};
use fieldsmith_doc::{Document, Face, Family, FillMode, Op, ShellSide};
use std::fmt::Write as _;

/// Helper functions shared by every generated field. Each mirrors a Rust function.
pub const PRELUDE: &str = r#"
float fs_sgn(float x) { return x < 0.0 ? -1.0 : 1.0; }
vec4 fs_min(vec4 a, vec4 b) { return a.x <= b.x ? a : b; }
vec4 fs_max(vec4 a, vec4 b) { return a.x >= b.x ? a : b; }
vec3 fs_norm(vec3 v) { float l = length(v); return l > 1e-12 ? v / l : vec3(0.0); }
float fs_blendk(vec4 a, vec4 b, float r) {
  if (r <= 0.0) return 0.0;
  return r * clamp(1.0 - dot(fs_norm(a.yzw), fs_norm(b.yzw)), 0.0, 1.0);
}
float fs_cgate(float xt, float yt, float x, float y, float k, float c) {
  float res = abs(xt - (k - y) * c) + abs(yt - (k - x) * c);
  return clamp(1.0 - res / (0.25 * k), 0.0, 1.0);
}
vec4 fs_rmin(vec4 a, vec4 b, float k) {
  if (k <= 1e-9) return fs_min(a, b);
  float u = max(k - a.x, 0.0);
  float v = max(k - b.x, 0.0);
  if (u <= 0.0 || v <= 0.0) return fs_min(a, b);
  float l = sqrt(u * u + v * v);
  return vec4(k - l, (a.yzw * u + b.yzw * v) / l);
}
vec4 fs_extrude(float wx, vec2 gx, float wy, float sz) {
  float ox = max(wx, 0.0); float oy = max(wy, 0.0);
  float ol = sqrt(ox * ox + oy * oy);
  if (ol > 0.0) { float cx = ox / ol; float cy = oy / ol; return vec4(ol, cx * gx.x, cx * gx.y, cy * sz); }
  if (wx > wy) return vec4(wx, gx.x, gx.y, 0.0);
  return vec4(wy, 0.0, 0.0, sz);
}
vec4 fs_sphere(vec3 p, float r) {
  float l = length(p);
  return vec4(l - r, l > 1e-12 ? p / l : vec3(0.0, 0.0, 1.0));
}
vec4 fs_box(vec3 p, vec3 hf, float rd) {
  vec3 q = abs(p) - (hf - vec3(rd));
  vec3 o = max(q, vec3(0.0));
  float ol = length(o);
  if (ol > 0.0) return vec4(ol - rd, vec3(fs_sgn(p.x), fs_sgn(p.y), fs_sgn(p.z)) * o / ol);
  if (q.y > q.x && q.y >= q.z) return vec4(q.y - rd, 0.0, fs_sgn(p.y), 0.0);
  if (q.z > q.x && q.z > q.y) return vec4(q.z - rd, 0.0, 0.0, fs_sgn(p.z));
  return vec4(q.x - rd, fs_sgn(p.x), 0.0, 0.0);
}
vec4 fs_cylinder(vec3 p, float r, float hh, float rd) {
  float rl = length(p.xy);
  vec2 u = rl > 1e-12 ? p.xy / rl : vec2(1.0, 0.0);
  vec4 d = fs_extrude(rl - (r - rd), u, abs(p.z) - (hh - rd), fs_sgn(p.z));
  return vec4(d.x - rd, d.yzw);
}
vec4 fs_capsule(vec3 p, float r, float hl) {
  vec3 q = vec3(p.xy, p.z - clamp(p.z, -hl, hl));
  float l = length(q);
  return vec4(l - r, l > 1e-12 ? q / l : vec3(1.0, 0.0, 0.0));
}
vec4 fs_torus(vec3 p, float R, float r) {
  float rl = length(p.xy);
  vec2 u = rl > 1e-12 ? p.xy / rl : vec2(1.0, 0.0);
  float qx = rl - R; float qz = p.z;
  float l = sqrt(qx * qx + qz * qz);
  vec2 c = l > 1e-12 ? vec2(qx, qz) / l : vec2(1.0, 0.0);
  return vec4(l - r, c.x * u, c.y);
}
mat3 fs_rot(int axis, float deg) {
  float a = radians(deg); float s = sin(a); float c = cos(a);
  if (axis == 0) return mat3(1.0, 0.0, 0.0, 0.0, c, s, 0.0, -s, c);
  if (axis == 1) return mat3(c, 0.0, -s, 0.0, 1.0, 0.0, s, 0.0, c);
  return mat3(c, s, 0.0, -s, c, 0.0, 0.0, 0.0, 1.0);
}
void fs_seg_line(vec2 p, vec2 a, vec2 b, inout float best, inout vec2 bv, inout bool inside) {
  vec2 e = b - a;
  if ((a.y > p.y) != (b.y > p.y)) { float x = a.x + (p.y - a.y) / (b.y - a.y) * e.x; if (p.x < x) inside = !inside; }
  float ee = dot(e, e);
  float t = ee > 0.0 ? clamp(dot(p - a, e) / ee, 0.0, 1.0) : 0.0;
  vec2 dv = p - (a + e * t);
  float dl = length(dv);
  if (dl < best) { best = dl; float el = max(length(e), 1e-12); bv = dl > 1e-12 ? dv / dl : vec2(e.y, -e.x) / el; }
}
void fs_seg_arc(vec2 p, vec2 a, vec2 b, float bulge, vec2 c, float r, vec2 md, float ch,
                inout float best, inout vec2 bv, inout bool inside) {
  vec2 e = b - a;
  if ((a.y > p.y) != (b.y > p.y)) { float x = a.x + (p.y - a.y) / (b.y - a.y) * e.x; if (p.x < x) inside = !inside; }
  vec2 v = p - c;
  float vl = length(v);
  vec2 cl;
  if (vl > 1e-12 && dot(v, md) / vl >= ch) { cl = c + v / vl * r; }
  else { cl = dot(p - a, p - a) <= dot(p - b, p - b) ? a : b; }
  float side = e.x * (p.y - a.y) - e.y * (p.x - a.x);
  if (vl < r && side * bulge < 0.0) inside = !inside;
  vec2 dv = p - cl;
  float dl = length(dv);
  if (dl < best) { best = dl; float el = max(length(e), 1e-12); bv = dl > 1e-12 ? dv / dl : vec2(e.y, -e.x) / el; }
}
vec4 fs_tpms(int fam, float a, vec3 p) {
  float k = 6.283185307179586 / a;
  float sx = sin(k * p.x); float cx = cos(k * p.x);
  float sy = sin(k * p.y); float cy = cos(k * p.y);
  float sz = sin(k * p.z); float cz = cos(k * p.z);
  if (fam == 0) return vec4(sx * cy + sy * cz + sz * cx, k * (cx * cy - sz * sx), k * (cy * cz - sx * sy), k * (cz * cx - sy * sz));
  if (fam == 1) return vec4(cx + cy + cz, -k * sx, -k * sy, -k * sz);
  return vec4(sx * sy * sz + sx * cy * cz + cx * sy * cz + cx * cy * sz,
    k * (cx * sy * sz + cx * cy * cz - sx * sy * cz - sx * cy * sz),
    k * (sx * cy * sz - sx * sy * cz + cx * cy * cz - cx * sy * sz),
    k * (sx * sy * cz - sx * cy * sz - cx * sy * sz + cx * cy * cz));
}
vec4 fs_tpms_distance(int fam, float a, vec3 p) {
  vec4 t = fs_tpms(fam, a, p);
  float gl = max(length(t.yzw), 0.3 * 6.283185307179586 / a);
  return t / gl;
}
"#;

/// Exact GLSL float literal for an f32 value.
fn lit(v: f64) -> String {
    let x = v as f32;
    if x == 0.0 {
        return "0.0".into();
    }
    let s = format!("{x:e}"); // e.g. "1.5e-3", "6e1", "-2.25e0"
    let (mant, exp) = s.split_once('e').unwrap();
    let mant = if mant.contains('.') {
        mant.to_string()
    } else {
        format!("{mant}.0")
    };
    if exp == "0" {
        mant
    } else {
        format!("{mant}e{exp}")
    }
}

fn v3(v: V3) -> String {
    format!("vec3({}, {}, {})", lit(v[0]), lit(v[1]), lit(v[2]))
}

/// GLSL `mat3` whose product with a vector equals `Rᵀ v` (columns are the rows of `r`).
fn mat_t(r: &M3) -> String {
    format!("mat3({}, {}, {})", v3(r[0]), v3(r[1]), v3(r[2]))
}

pub struct Generated {
    pub source: String,
    /// Density / parameter grid to upload as `u_param` (R32F 3D texture), if the field has a fill.
    pub grid: Option<crate::lattice::Grid3>,
    /// Mesh textures (`u_mesh{slot}`), in slot order. Data is fetched from the meshfield
    /// registry by `key` at the bind site.
    pub meshes: Vec<MeshTex>,
    /// The slot map used (key → slot). A caller that glues a second pass into the same
    /// fragment program — the viewport's `fs_ident` — must number keys the same, so it
    /// takes the map back from here ([`ident_with_slots`]).
    pub slots: MeshSlots,
}

/// One baked mesh field referenced by a generated program.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct MeshTex {
    /// Slot number: uniform `u_mesh{slot}` (distance) / `u_mesh{slot}p` (patch).
    pub slot: usize,
    /// Registry key (content hash), also the sidecar entry name.
    pub key: u64,
    pub dims: [usize; 3],
    pub origin: crate::dual::V3,
    pub spacing: f64,
    /// A patch grid accompanies the distance grid (`u_mesh{slot}p`, UI16 texture).
    pub patch: bool,
}

/// Per-slot specs a Gen may emit; index = slot.
type MeshSlots = std::collections::BTreeMap<u64, usize>;

struct Gen {
    out: String,
    next: usize,
    prefix: String,
    grid: Option<crate::lattice::Grid3>,
    /// Slot map for mesh keys (see `crate::expr::mesh_keys`).
    slots: MeshSlots,
    /// Emitted mesh state, indexed by slot.
    meshes: Vec<Option<crate::meshfield::GridSpec>>,
    /// Whether mesh samples may read textures (the field programs); false makes mesh leaves
    /// emit vacuum so overlay programs never reference an unbound sampler.
    sample_mesh: bool,
    /// Whether `face_fn`'s mesh branch may emit an ident referencing the field program's
    /// `fs_mesh{slot}` and `u_mesh{slot}p` — the ident pass is glued into the same fragment
    /// program after the body's declarations, so it references but never re-emits them.
    /// Only `ident()` sets this; overlay (a standalone program) keeps mesh leaves on vacuum.
    mesh_idents: bool,
}

impl Gen {
    fn new(prefix: &str, slots: &MeshSlots, sample_mesh: bool, mesh_idents: bool) -> Self {
        Gen {
            out: String::new(),
            next: 0,
            prefix: prefix.into(),
            grid: None,
            slots: slots.clone(),
            meshes: vec![None; slots.len()],
            sample_mesh,
            mesh_idents,
        }
    }

    fn fresh(&mut self) -> String {
        let name = format!("{}{}", self.prefix, self.next);
        self.next += 1;
        name
    }

    fn func(&mut self, body: String) -> String {
        let name = self.fresh();
        let _ = writeln!(self.out, "vec4 {name}(vec3 p) {{\n{body}}}");
        name
    }

    /// Slot for a hydrated mesh key, emitting `fs_mesh{slot}` (distance + analytic trilinear
    /// gradient) on first use. Only reachable when `sample_mesh` is set.
    fn mesh_slot(&mut self, key: u64, spec: &crate::meshfield::GridSpec) -> usize {
        let slot = self.slots[&key];
        if self.meshes[slot].is_none() {
            self.meshes[slot] = Some(spec.clone());
            self.emit_mesh_fn(slot, spec);
        }
        slot
    }

    /// `u_mesh{slot}` is an R32F sampler3D with the grid's node-centred samples. Texel
    /// selection and slope formulas mirror [`crate::lattice::Grid3::sample_grad`] — the
    /// grid never has a 1-node axis (voxeliser guarantee), so `i0 + 1` stays in bounds.
    /// A patch grid travelling with the mesh declares `u_mesh{slot}p` (usampler3D, UI16)
    /// next to the distance sampler so the ident pass, which is glued into the same
    /// fragment program after these declarations, can fetch it without redeclaring.
    fn emit_mesh_fn(&mut self, slot: usize, s: &crate::meshfield::GridSpec) {
        let patch_decl = if s.patch {
            format!("uniform highp usampler3D u_mesh{slot}p;\n")
        } else {
            String::new()
        };
        let _ = writeln!(
            self.out,
            "{patch_decl}uniform highp sampler3D u_mesh{slot};\n\
vec4 fs_mesh{slot}(vec3 p) {{\n  \
vec3 n = vec3({}, {}, {});\n  \
vec3 u = clamp((p - {}) / {}, vec3(0.0), n - 1.0);\n  \
ivec3 i0 = ivec3(min(floor(u), n - 2.0));\n  \
vec3 f = u - vec3(i0);\n  \
float c000 = texelFetch(u_mesh{slot}, i0, 0).r; float c100 = texelFetch(u_mesh{slot}, i0 + ivec3(1, 0, 0), 0).r;\n  \
float c010 = texelFetch(u_mesh{slot}, i0 + ivec3(0, 1, 0), 0).r; float c110 = texelFetch(u_mesh{slot}, i0 + ivec3(1, 1, 0), 0).r;\n  \
float c001 = texelFetch(u_mesh{slot}, i0 + ivec3(0, 0, 1), 0).r; float c101 = texelFetch(u_mesh{slot}, i0 + ivec3(1, 0, 1), 0).r;\n  \
float c011 = texelFetch(u_mesh{slot}, i0 + ivec3(0, 1, 1), 0).r; float c111 = texelFetch(u_mesh{slot}, i0 + ivec3(1, 1, 1), 0).r;\n  \
float c00 = c000 * (1.0 - f.x) + c100 * f.x; float c10 = c010 * (1.0 - f.x) + c110 * f.x;\n  \
float c01 = c001 * (1.0 - f.x) + c101 * f.x; float c11 = c011 * (1.0 - f.x) + c111 * f.x;\n  \
float c0 = c00 * (1.0 - f.y) + c10 * f.y; float c1 = c01 * (1.0 - f.y) + c11 * f.y;\n  \
float v = c0 * (1.0 - f.z) + c1 * f.z;\n  \
float gx = ((c100 - c000) * (1.0 - f.y) + (c110 - c010) * f.y) * (1.0 - f.z) + ((c101 - c001) * (1.0 - f.y) + (c111 - c011) * f.y) * f.z;\n  \
float gy = ((c010 - c000) * (1.0 - f.x) + (c110 - c100) * f.x) * (1.0 - f.z) + ((c011 - c001) * (1.0 - f.x) + (c111 - c101) * f.x) * f.z;\n  \
float gz = c1 - c0;\n  \
return vec4(v, gx / {}, gy / {}, gz / {});\n}}",
            lit(s.dims[0] as f64),
            lit(s.dims[1] as f64),
            lit(s.dims[2] as f64),
            v3(s.origin),
            lit(s.spacing),
            lit(s.spacing),
            lit(s.spacing),
            lit(s.spacing)
        );
    }

    /// Emit `vec3 name(vec2 q)` returning (signed 2D distance, gradient) for a profile.
    fn profile(&mut self, segs: &[crate::prim::ArcSeg]) -> String {
        let name = self.fresh();
        let mut b =
            String::from("  float best = 1e30; vec2 bv = vec2(1.0, 0.0); bool inside = false;\n");
        for s in segs {
            let a = format!("vec2({}, {})", lit(s.a[0]), lit(s.a[1]));
            let bb = format!("vec2({}, {})", lit(s.b[0]), lit(s.b[1]));
            if s.bulge == 0.0 {
                let _ = writeln!(b, "  fs_seg_line(q, {a}, {bb}, best, bv, inside);");
            } else {
                let (c, r, mid, ch) = arc_geom(s);
                let md = [(mid[0] - c[0]) / r, (mid[1] - c[1]) / r];
                let _ = writeln!(
                    b,
                    "  fs_seg_arc(q, {a}, {bb}, {}, vec2({}, {}), {}, vec2({}, {}), {}, best, bv, inside);",
                    lit(s.bulge),
                    lit(c[0]),
                    lit(c[1]),
                    lit(r),
                    lit(md[0]),
                    lit(md[1]),
                    lit(ch)
                );
            }
        }
        b.push_str("  return inside ? vec3(-best, -bv) : vec3(best, bv);\n");
        let _ = writeln!(self.out, "vec3 {name}(vec2 q) {{\n{b}}}");
        name
    }

    fn expr(&mut self, e: &Expr) -> String {
        match e {
            Expr::Empty => self.func("  return vec4(1e9, 0.0, 0.0, 0.0);\n".into()),
            Expr::Prim(p) => {
                let body = match p {
                    Prim::Sphere { radius } => {
                        format!("  return fs_sphere(p, {});\n", lit(*radius))
                    }
                    Prim::Box { half, round } => {
                        format!("  return fs_box(p, {}, {});\n", v3(*half), lit(*round))
                    }
                    Prim::Cylinder {
                        radius,
                        half_h,
                        round,
                    } => format!(
                        "  return fs_cylinder(p, {}, {}, {});\n",
                        lit(*radius),
                        lit(*half_h),
                        lit(*round)
                    ),
                    Prim::Capsule { radius, half_len } => {
                        format!(
                            "  return fs_capsule(p, {}, {});\n",
                            lit(*radius),
                            lit(*half_len)
                        )
                    }
                    Prim::Torus { major, minor } => {
                        format!("  return fs_torus(p, {}, {});\n", lit(*major), lit(*minor))
                    }
                    Prim::Extrude { segs, half_h } => {
                        let pf = self.profile(segs);
                        format!(
                            "  vec3 s = {pf}(p.xy);\n  return fs_extrude(s.x, s.yz, abs(p.z) - {}, fs_sgn(p.z));\n",
                            lit(*half_h)
                        )
                    }
                };
                self.func(body)
            }
            Expr::Xform { m, e } => {
                let c = self.expr(e);
                self.func(format!(
                    "  mat3 L = {};\n  vec4 d = {c}(L * (p - {}));\n  return vec4(d.x, d.yzw * L);\n",
                    mat_t(&m.r),
                    v3(m.t)
                ))
            }
            Expr::Combine { op, blend, a, b } => {
                let ca = self.expr(a);
                let cb = self.expr(b);
                let body = if *blend <= 0.0 {
                    match op {
                        Op::Add => format!("  return fs_min({ca}(p), {cb}(p));\n"),
                        Op::Subtract => format!("  return fs_max({ca}(p), -{cb}(p));\n"),
                        Op::Intersect => format!("  return fs_max({ca}(p), {cb}(p));\n"),
                    }
                } else {
                    let r = lit(*blend);
                    let (x, y, s, sx, sy) = match op {
                        Op::Add => ("da", "db", "", "", ""),
                        Op::Subtract => ("-da", "db", "-", "-", ""),
                        Op::Intersect => ("-da", "-db", "-", "-", "-"),
                    };
                    format!(
                        "  vec4 da = {ca}(p);\n  vec4 db = {cb}(p);\n  vec4 x = {x};\n  vec4 y = {y};\n  \
                         float k = fs_blendk(x, y, {r});\n  \
                         if (k > 0.0 && x.x < k && y.x < k) {{\n    \
                         vec3 nx = fs_norm(x.yzw); vec3 ny = fs_norm(y.yzw);\n    \
                         vec3 c = p + nx * (k - x.x) + ny * (k - y.x);\n    \
                         k *= fs_cgate({sx}{ca}(c - nx * k).x, {sy}{cb}(c - ny * k).x, x.x, y.x, k, dot(nx, ny));\n  }}\n  \
                         return {s}fs_rmin(x, y, k);\n"
                    )
                };
                self.func(body)
            }
            Expr::Shell { side, t, e } => {
                let c = self.expr(e);
                let t = lit(*t);
                let body = match side {
                    ShellSide::Inside => format!("  vec4 d = {c}(p);\n  return fs_max(d, vec4(-(d.x + {t}), -d.yzw));\n"),
                    ShellSide::Outside => format!("  vec4 d = {c}(p);\n  return fs_max(vec4(d.x - {t}, d.yzw), -d);\n"),
                    ShellSide::Centre => format!(
                        "  vec4 d = {c}(p);\n  float s = d.x < 0.0 ? -1.0 : 1.0;\n  return vec4(abs(d.x) - 0.5 * {t}, d.yzw * s);\n"
                    ),
                };
                self.func(body)
            }
            Expr::Offset { d, e } => {
                let c = self.expr(e);
                self.func(format!(
                    "  vec4 d = {c}(p);\n  return vec4(d.x - {}, d.yzw);\n",
                    lit(*d)
                ))
            }
            Expr::Mirror { axis, e } => {
                let c = self.expr(e);
                let m = [
                    "vec3(-1.0, 1.0, 1.0)",
                    "vec3(1.0, -1.0, 1.0)",
                    "vec3(1.0, 1.0, -1.0)",
                ][*axis];
                self.func(format!(
                    "  vec4 a = {c}(p);\n  vec4 b = {c}(p * {m});\n  return fs_min(a, vec4(b.x, b.yzw * {m}));\n"
                ))
            }
            Expr::Linear { count, step, e } => {
                let c = self.expr(e);
                self.func(format!(
                    "  vec4 best = vec4(1e9, 0.0, 0.0, 0.0);\n  for (int i = 0; i < {count}; i++) {{ best = fs_min(best, {c}(p - {} * float(i))); }}\n  return best;\n",
                    v3(*step)
                ))
            }
            Expr::Circular {
                count,
                axis,
                step_deg,
                e,
            } => {
                let c = self.expr(e);
                self.func(format!(
                    "  vec4 best = vec4(1e9, 0.0, 0.0, 0.0);\n  for (int i = 0; i < {count}; i++) {{\n    mat3 R = fs_rot({axis}, {} * float(i));\n    vec4 d = {c}(transpose(R) * p);\n    best = fs_min(best, vec4(d.x, R * d.yzw));\n  }}\n  return best;\n",
                    lit(*step_deg)
                ))
            }
            Expr::Filled(fx) => self.filled(fx),
            Expr::Mesh { key, spec, .. } => match (self.sample_mesh, spec) {
                (_, None) | (false, _) => self.func("  return vec4(1e9, 0.0, 0.0, 0.0);\n".into()),
                (true, Some(spec)) => {
                    let slot = self.mesh_slot(*key, spec);
                    self.func(format!("  return fs_mesh{slot}(p);\n"))
                }
            },
        }
    }

    fn filled(&mut self, fx: &FillExpr) -> String {
        let body = self.expr(&fx.body);
        let keep = if matches!(fx.keep, Expr::Empty) {
            None
        } else {
            Some(self.expr(&fx.keep))
        };
        let param_expr = self.param_term(&fx.param);
        let lattice = lattice_term(fx.family, fx.mode, fx.cell, &param_expr);
        let mut b = format!("  vec4 d = {body}(p);\n  vec4 core = vec4(-(d.x + {}), -d.yzw);\n{lattice}  vec4 inner = fs_min(core, lat);\n", lit(fx.skin));
        if let Some(k) = keep {
            let _ = writeln!(b, "  inner = fs_min(inner, {k}(p));");
        }
        b.push_str("  return fs_max(d, inner);\n");
        self.func(b)
    }

    /// `vec4 fs_core(vec3 p)`: the fill with the outer skin removed — what
    /// [`generate_viewport`] appends for a filled document. Mirrors `eval` of the
    /// shell-band fill with the band term exchanged for the shifted clip `d + skin` (see
    /// [`crate::lattice::FillExpr::removed_skin`], the Rust twin). `body_fn` is the body
    /// field's emitted `vec4 fs_n…(vec3 p)` name — usually the same function `fs_field`
    /// roots on, reused here instead of re-emitting the body. Emitted under the fixed
    /// name `fs_core` (not a numbered `fs_n…`) so the JS pass template can declare and
    /// call it without carrying the name across.
    fn core_fn(&mut self, fx: &FillExpr, body_fn: &str) -> String {
        let keep = if matches!(fx.keep, Expr::Empty) {
            None
        } else {
            Some(self.expr(&fx.keep))
        };
        let param_expr = self.param_term(&fx.param);
        let lattice = lattice_term(fx.family, fx.mode, fx.cell, &param_expr);
        let inner = match keep {
            Some(k) => format!("  vec4 inner = fs_min(lat, {k}(p));\n"),
            None => "  vec4 inner = lat;\n".to_string(),
        };
        let b = format!(
            "  vec4 d = {body_fn}(p);\n{lattice}{inner}  return fs_max(vec4(d.x + {}, d.yzw), inner);\n",
            lit(fx.skin)
        );
        let _ = writeln!(self.out, "vec4 fs_core(vec3 p) {{\n{b}}}");
        "fs_core".into()
    }

    /// Declare `u_param` + emit `fs_grid` when the parameter grid has more than one node,
    /// and return the in-shader expression for the parameter at `p`; a single-node grid
    /// embeds the constant as a GLSL literal instead. The caller of an emitted grid
    /// uploads its data as the R32F `u_param` texture (`Generated.grid`).
    fn param_term(&mut self, grid: &crate::lattice::Grid3) -> String {
        if grid.data.len() == 1 {
            return lit(grid.data[0] as f64);
        }
        self.grid = Some(grid.clone());
        let _ = writeln!(
            self.out,
            "uniform highp sampler3D u_param;\nfloat fs_grid(vec3 p) {{\n  vec3 n = vec3({}, {}, {});\n  vec3 u = clamp((p - {}) / {}, vec3(0.0), n - 1.0);\n  ivec3 i0 = ivec3(min(floor(u), n - 2.0));\n  vec3 f = u - vec3(i0);\n  float c000 = texelFetch(u_param, i0, 0).r; float c100 = texelFetch(u_param, i0 + ivec3(1, 0, 0), 0).r;\n  float c010 = texelFetch(u_param, i0 + ivec3(0, 1, 0), 0).r; float c110 = texelFetch(u_param, i0 + ivec3(1, 1, 0), 0).r;\n  float c001 = texelFetch(u_param, i0 + ivec3(0, 0, 1), 0).r; float c101 = texelFetch(u_param, i0 + ivec3(1, 0, 1), 0).r;\n  float c011 = texelFetch(u_param, i0 + ivec3(0, 1, 1), 0).r; float c111 = texelFetch(u_param, i0 + ivec3(1, 1, 1), 0).r;\n  float c00 = c000 * (1.0 - f.x) + c100 * f.x; float c10 = c010 * (1.0 - f.x) + c110 * f.x;\n  float c01 = c001 * (1.0 - f.x) + c101 * f.x; float c11 = c011 * (1.0 - f.x) + c111 * f.x;\n  float c0 = c00 * (1.0 - f.y) + c10 * f.y; float c1 = c01 * (1.0 - f.y) + c11 * f.y;\n  return c0 * (1.0 - f.z) + c1 * f.z;\n}}",
            lit(grid.dims[0].max(2) as f64),
            lit(grid.dims[1].max(2) as f64),
            lit(grid.dims[2].max(2) as f64),
            v3(grid.origin),
            lit(grid.spacing)
        );
        "fs_grid(p)".into()
    }
}

/// The lattice sampler `vec4 lat = …` lines, in the mode's wall semantics (sheet walls thin
/// by ±param, network struts thin by param), on the TPMS surfaces of `family`.
fn lattice_term(family: Family, mode: FillMode, cell: f64, param_expr: &str) -> String {
    let fam = match family {
        Family::Gyroid => 0,
        Family::Primitive => 1,
        Family::Diamond => 2,
    };
    match mode {
        FillMode::Sheet => format!(
            "  vec4 s = fs_tpms_distance({fam}, {cell}, p);\n  float sg = s.x < 0.0 ? -1.0 : 1.0;\n  vec4 lat = vec4(abs(s.x) - 0.5 * {param_expr}, s.yzw * sg);\n",
            cell = lit(cell)
        ),
        FillMode::Network => format!(
            "  vec4 s = fs_tpms_distance({fam}, {cell}, p);\n  vec4 lat = vec4(s.x - {param_expr}, s.yzw);\n",
            cell = lit(cell)
        ),
    }
}

/// Mesh keys referenced by the given expressions, ranked into slots (deterministic within
/// a program regardless of traversal order).
fn mesh_slots<'a>(exprs: impl IntoIterator<Item = &'a Expr>) -> MeshSlots {
    let mut keys = std::collections::BTreeSet::new();
    for e in exprs {
        crate::meshfield::mesh_keys(e, &mut keys);
    }
    keys.into_iter().enumerate().map(|(i, k)| (k, i)).collect()
}

/// Generate the GLSL for `e`: prelude, node functions and `vec4 fs_field(vec3 p)`.
pub fn generate(e: &Expr) -> Generated {
    let slots = mesh_slots(std::iter::once(e));
    let keys: Vec<u64> = slots.keys().copied().collect();
    let mut g = Gen::new("fs_n", &slots, true, false);
    let root = g.expr(e);
    let mut source = String::from(PRELUDE);
    source.push_str(&g.out);
    let _ = writeln!(source, "vec4 fs_field(vec3 p) {{ return {root}(p); }}");
    let meshes = g
        .meshes
        .iter()
        .enumerate()
        .filter_map(|(slot, spec)| {
            spec.as_ref().map(|s| MeshTex {
                slot,
                key: keys[slot],
                dims: s.dims,
                origin: s.origin,
                spacing: s.spacing,
                patch: s.patch,
            })
        })
        .collect();
    Generated {
        source,
        grid: g.grid,
        meshes,
        slots,
    }
}

/// Index of a face in the GLSL face encoding (matches `Face::ALL` order).
pub fn face_code(f: Face) -> usize {
    Face::ALL.iter().position(|x| *x == f).unwrap()
}

/// Generate the viewport's field program: the body field as `vec4 fs_field(vec3 p)` —
/// byte-identical to [`generate`] of the same body, so the opaque/silhouette view and the
/// uniform gate the JS side has today are unaffected — plus, when the document fills,
/// `vec4 fs_core(vec3 p)` for the same program: the fill with the outer skin removed
/// (lattice struts and protected zones; nothing within one `skin` of the body surface),
/// what the material shown through a semi-transparent body renders. One shared program
/// compiles once; `u_param`/`fs_grid` arrive once and only `fs_core` references them.
pub fn generate_viewport(body: &Expr, fill: Option<&FillExpr>) -> Generated {
    // Slot keys from every referenced expression, so a mesh leaf reachable only through a
    // fill's protected zone (or body) still numbers its `fs_mesh{slot}` deterministically;
    // mesh_keys collects a set, so order-independent.
    let mut referenced: Vec<&Expr> = vec![body];
    if let Some(fx) = fill {
        referenced.push(&fx.body);
        referenced.push(&fx.keep);
    }
    let slots = mesh_slots(referenced);
    let keys: Vec<u64> = slots.keys().copied().collect();
    let mut g = Gen::new("fs_n", &slots, true, false);
    let root = g.expr(body);
    // When the fill's body is the same expression (it always is from compile_body), the
    // core pass reuses the emitted function instead of duplicating every node function.
    if let Some(fx) = fill {
        let body_fn = if fx.body == *body {
            root.clone()
        } else {
            g.expr(&fx.body)
        };
        g.core_fn(fx, &body_fn);
    }
    let mut source = String::from(PRELUDE);
    source.push_str(&g.out);
    let _ = writeln!(source, "vec4 fs_field(vec3 p) {{ return {root}(p); }}");
    let meshes = g
        .meshes
        .iter()
        .enumerate()
        .filter_map(|(slot, spec)| {
            spec.as_ref().map(|s| MeshTex {
                slot,
                key: keys[slot],
                dims: s.dims,
                origin: s.origin,
                spacing: s.spacing,
                patch: s.patch,
            })
        })
        .collect();
    Generated {
        source,
        grid: g.grid,
        meshes,
        slots,
    }
}

impl Gen {
    /// Emit `vec2 name(vec3 p)` = (signed distance, face code) for a placed shape, mirroring
    /// [`crate::expr::eval_face`].
    fn face_fn(&mut self, e: &Expr) -> String {
        let fc = |f: Face| face_code(f);
        let body = match e {
            Expr::Prim(prim) => {
                let v = self.expr(e);
                match prim {
                    Prim::Box { half, .. } => format!(
                        "  vec3 q = abs(p) - {};\n  int i = 0; if (q.y > q.x) i = 1; if (q.z > (i == 0 ? q.x : q.y)) i = 2;\n  float f = i == 0 ? (p.x >= 0.0 ? {px}.0 : {nx}.0) : i == 1 ? (p.y >= 0.0 ? {py}.0 : {ny}.0) : (p.z >= 0.0 ? {pz}.0 : {nz}.0);\n  return vec2({v}(p).x, f);\n",
                        v3(*half),
                        px = fc(Face::Px),
                        nx = fc(Face::Nx),
                        py = fc(Face::Py),
                        ny = fc(Face::Ny),
                        pz = fc(Face::Pz),
                        nz = fc(Face::Nz)
                    ),
                    Prim::Cylinder { radius, half_h, .. } => format!(
                        "  float side = length(p.xy) - {}; float cap = abs(p.z) - {};\n  float f = side >= cap ? {s}.0 : (p.z >= 0.0 ? {t}.0 : {b}.0);\n  return vec2({v}(p).x, f);\n",
                        lit(*radius),
                        lit(*half_h),
                        s = fc(Face::Side),
                        t = fc(Face::Top),
                        b = fc(Face::Bottom)
                    ),
                    Prim::Extrude { segs, half_h } => {
                        let pf = self.profile(segs);
                        format!(
                            "  float side = {pf}(p.xy).x; float cap = abs(p.z) - {};\n  float f = side >= cap ? {s}.0 : (p.z >= 0.0 ? {t}.0 : {b}.0);\n  return vec2({v}(p).x, f);\n",
                            lit(*half_h),
                            s = fc(Face::Side),
                            t = fc(Face::Top),
                            b = fc(Face::Bottom)
                        )
                    }
                    _ => format!("  return vec2({v}(p).x, 0.0);\n"),
                }
            }
            Expr::Xform { m, e } => {
                let c = self.face_fn(e);
                format!("  return {c}({} * (p - {}));\n", mat_t(&m.r), v3(m.t))
            }
            // Mesh leaves mirror `eval_face` (expr.rs): the distance comes from the field
            // program's `fs_mesh{slot}`, referenced here but never re-emitted — the ident
            // source is glued into the same fragment program after the body (viewport.ts
            // fragSource), with the slot map derived from the same `compile_body` (ident).
            // The face code: 0 (Face::All) for an unpatched mesh; otherwise `PATCH_CODE0`
            // plus the patch id of the nearest grid cell, the cell math being exactly
            // `MeshField::cell_of` (node-centred coordinate clamped to the last node, then
            // floored — always in bounds, so texelFetch never leaves the patch grid). A
            // cell without a claim stores NO_PATCH (65535), reporting code 10 + 65535,
            // which no selector can name (ids cap at 65524), so unclaimed surface simply
            // never highlights. CPU picking keeps reporting face "all" for mesh hits and
            // carries the patch itself (`pick.patch`); the numeric GPU↔CPU agreement (a
            // clicked patch highlights exactly its cells) is measured in step 9's Playwright
            // e2e — cargo cannot execute WebGL.
            Expr::Mesh { key, spec, .. } => match (self.mesh_idents, spec) {
                (false, _) | (true, None) => "  return vec2(1e9, 0.0);\n".into(),
                (true, Some(s)) if !s.patch => {
                    let slot = self.slots[key];
                    format!("  return vec2(fs_mesh{slot}(p).x, 0.0);\n")
                }
                (true, Some(s)) => {
                    let slot = self.slots[key];
                    format!(
"  vec3 n = vec3({}, {}, {});\n  vec3 u = clamp((p - {}) / {}, vec3(0.0), n - 1.0);\n  return vec2(fs_mesh{slot}(p).x, {} + float(texelFetch(u_mesh{slot}p, ivec3(floor(u)), 0).r));\n",
                        lit(s.dims[0] as f64),
                        lit(s.dims[1] as f64),
                        lit(s.dims[2] as f64),
                        v3(s.origin),
                        lit(s.spacing),
                        lit(crate::meshfield::PATCH_CODE0 as f64),
                    )
                }
            },
            Expr::Mirror { axis, e } => {
                let c = self.face_fn(e);
                let m = [
                    "vec3(-1.0, 1.0, 1.0)",
                    "vec3(1.0, -1.0, 1.0)",
                    "vec3(1.0, 1.0, -1.0)",
                ][*axis];
                format!("  vec2 a = {c}(p); vec2 b = {c}(p * {m});\n  return abs(a.x) <= abs(b.x) ? a : b;\n")
            }
            Expr::Linear { count, step, e } => {
                let c = self.face_fn(e);
                format!(
                    "  vec2 best = vec2(1e9, 0.0);\n  for (int i = 0; i < {count}; i++) {{ vec2 d = {c}(p - {} * float(i)); if (abs(d.x) < abs(best.x)) best = d; }}\n  return best;\n",
                    v3(*step)
                )
            }
            Expr::Circular {
                count,
                axis,
                step_deg,
                e,
            } => {
                let c = self.face_fn(e);
                format!(
                    "  vec2 best = vec2(1e9, 0.0);\n  for (int i = 0; i < {count}; i++) {{ vec2 d = {c}(transpose(fs_rot({axis}, {} * float(i))) * p); if (abs(d.x) < abs(best.x)) best = d; }}\n  return best;\n",
                    lit(*step_deg)
                )
            }
            // Composites mirror `eval_face`'s Combine arm (expr.rs): the nearer operand's
            // face function wins — argmin |d|, ties to `a` — and a blend fillet band
            // returns the composed distance with code 0 (the body's own fs program,
            // referenced here; it re-emits its operands' subprograms, which the no-dedup
            // emitter tolerates as separate names).
            Expr::Combine { op: _, blend, a, b } => {
                let ca = self.face_fn(a);
                let cb = self.face_fn(b);
                let band = if *blend > 0.0 {
                    let v = self.expr(e);
                    format!(
                        "  if (abs(da.x) < {r} && abs(db.x) < {r}) return vec2({v}(p).x, 0.0);\n",
                        r = lit(*blend)
                    )
                } else {
                    String::new()
                };
                format!(
                    "  vec2 da = {ca}(p); vec2 db = {cb}(p);\n{band}  return abs(db.x) < abs(da.x) ? db : da;\n"
                )
            }
            other => {
                let v = self.expr(other);
                format!("  return vec2({v}(p).x, 0.0);\n")
            }
        };
        let name = self.fresh();
        let _ = writeln!(self.out, "vec2 {name}(vec3 p) {{\n{body}}}");
        name
    }
}

/// One coloured overlay: the union of some shape faces.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct OverlayItem {
    pub selectors: Vec<fieldsmith_doc::Selector>,
    pub color: [f64; 3],
}

/// Emit `vec4 fs_overlay(vec3 p, float eps)` returning `(rgb, alpha)` for the first-listed-last-wins
/// overlay whose surface lies within `eps` of `p`. Missing shapes are skipped.
pub fn overlay(doc: &Document, items: &[OverlayItem]) -> String {
    // Slot map over every placed shape the program touches. Overlay stays a standalone
    // program: mesh leaves degrade to vacuum here (patch-aware overlays would need their
    // own texture uploads, deferred until a use asks for them).
    let shapes: Vec<Expr> = items
        .iter()
        .flat_map(|item| item.selectors.iter())
        .filter_map(|sel| placed_shape(doc, &sel.shape))
        .collect();
    let slots = mesh_slots(shapes.iter());
    let mut g = Gen::new("fs_o", &slots, false, false);
    let mut checks = String::new();
    for item in items {
        for sel in &item.selectors {
            let Some(e) = placed_shape(doc, &sel.shape) else {
                continue;
            };
            let f = g.face_fn(&e);
            let cond = if sel.face == Face::All {
                String::new()
            } else {
                format!(" && abs(r.y - {}.0) < 0.5", face_code(sel.face))
            };
            let _ = writeln!(
                checks,
                "  r = {f}(p); if (abs(r.x) < eps{cond}) o = vec4({}, {}, {}, 1.0);",
                lit(item.color[0]),
                lit(item.color[1]),
                lit(item.color[2])
            );
        }
    }
    let mut s = g.out;
    let _ = writeln!(s, "vec4 fs_overlay(vec3 p, float eps) {{\n  vec4 o = vec4(0.0); vec2 r;\n{checks}  return o;\n}}");
    s
}

/// Emit `vec2 fs_ident(vec3 p)` = (leaf index, face code) of the active leaf whose placed surface
/// is nearest to `p`, mirroring [`crate::expr::identify_surface`]. Returns the GLSL and the leaf
/// ids in index order. Used for selection and hover highlighting driven by uniforms.
pub fn ident(doc: &Document) -> (String, Vec<String>) {
    // The slot map must come from the same body expression the `generate` call used, not
    // from the active-leaf pairs below: slots are the sorted-position of their key over the
    // whole set, and active leaves are a subset of the body — deriving slots from the subset
    // could number a key differently in `fs_ident` than in the body's `fs_mesh{slot}`, and
    // the glued program would reference an unemitted function. Compiling the body again is
    // cheap next to the GLSL compile on the GPU.
    let body = crate::expr::compile_body(doc);
    let slots = mesh_slots(std::iter::once(&body));
    ident_with_slots(doc, &slots)
}

/// [`ident`] with the caller's slot map. The viewport program derives its slots over the
/// body ∪ fill-referenced keys ([`generate_viewport`]), so the ident pass glued into that
/// program must number them the same — the caller passes `Generated.slots` back in.
pub fn ident_with_slots(doc: &Document, slots: &MeshSlots) -> (String, Vec<String>) {
    let pairs: Vec<(String, Expr)> = crate::expr::active_leaves(doc)
        .into_iter()
        .filter_map(|id| {
            let e = placed_shape(doc, &id)?;
            Some((id, e))
        })
        .collect();
    let mut g = Gen::new("fs_i", slots, false, true);
    let mut body = String::from("  vec2 best = vec2(1e9, 0.0); float idx = -1.0; vec2 r;\n");
    let mut ids = Vec::new();
    for (id, e) in &pairs {
        let f = g.face_fn(e);
        let _ = writeln!(
            body,
            "  r = {f}(p); if (abs(r.x) < abs(best.x)) {{ best = r; idx = {}.0; }}",
            ids.len()
        );
        ids.push(id.clone());
    }
    body.push_str("  return vec2(idx, best.y);\n");
    let mut s = g.out;
    let _ = writeln!(s, "vec2 fs_ident(vec3 p) {{\n{body}}}");
    (s, ids)
}

/// Emit `vec4 <name>(vec3 p)` for an arbitrary expression without the prelude (for extra fields
/// in a shader that already contains [`PRELUDE`]). Mesh-containing fields must go through
/// [`generate`] — the caller here would not bind the mesh textures.
pub fn generate_named(e: &Expr, prefix: &str, name: &str) -> String {
    let slots = mesh_slots(std::iter::once(e));
    let mut g = Gen::new(prefix, &slots, true, false);
    let root = g.expr(e);
    let mut s = g.out;
    let _ = writeln!(s, "vec4 {name}(vec3 p) {{ return {root}(p); }}");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_are_valid_glsl_floats() {
        assert_eq!(lit(0.0), "0.0");
        assert_eq!(lit(1.0), "1.0");
        assert_eq!(lit(60.0), "6.0e1");
        assert_eq!(lit(-0.001), "-1.0e-3");
        assert_eq!(lit(3.25), "3.25");
    }

    /// A representative fill (sheet and network, one with a protected zone) for the
    /// viewport-program tests below.
    fn filled_expr(mode: FillMode, keep: Expr) -> (Expr, FillExpr) {
        let body = Expr::Prim(Prim::Box {
            half: [30.0, 20.0, 4.0],
            round: 0.0,
        });
        let fx = FillExpr {
            body: body.clone(),
            skin: 1.2,
            family: Family::Gyroid,
            mode,
            cell: 6.0,
            param: crate::lattice::Grid3 {
                origin: [0.0; 3],
                spacing: 1.0,
                dims: [7, 5, 4],
                data: (0..140).map(|i| (i as f32) * 0.01).collect(),
            },
            keep,
        };
        (body, fx)
    }

    /// The body pass's field is unaffected by a fill: with no fill the program is
    /// `generate` of the body; with a fill, every line of the body program is still in the
    /// shared program verbatim, alongside the appended core piece.
    #[test]
    fn viewport_program_keeps_the_body_field() {
        let body = Expr::Prim(Prim::Box {
            half: [30.0, 20.0, 4.0],
            round: 0.0,
        });
        assert_eq!(
            generate_viewport(&body, None).source,
            generate(&body).source,
            "no fill: the viewport program is the body program"
        );
        for (mode, keep) in [
            (FillMode::Sheet, Expr::Empty),
            (
                FillMode::Network,
                Expr::Offset {
                    d: 1.0,
                    e: Box::new(Expr::Prim(Prim::Sphere { radius: 5.0 })),
                },
            ),
        ] {
            let (body, fx) = filled_expr(mode, keep);
            let g = generate_viewport(&body, Some(&fx));
            for plain_line in generate(&body).source.lines().filter(|l| !l.trim().is_empty()) {
                assert!(
                    g.source.contains(plain_line),
                    "{mode:?}: body line missing from the shared program: {plain_line}"
                );
            }
            assert!(
                g.source.contains("vec4 fs_core(vec3 p) {"),
                "{mode:?}: the core piece is in the program"
            );
            assert!(
                g.source.contains("fs_max(vec4(d.x + 1.2"),
                "{mode:?}: the core piece clips one skin depth inside"
            );
            assert!(
                !g.source.contains("vec4 core = vec4(-(d.x"),
                "{mode:?}: no shell-band term in the core piece"
            );
        }
    }

    /// One `u_param` declaration in the shared program for a graded grid, none for a
    /// single-node grid (a literal); `Generated.grid` rides only when a texture is needed.
    #[test]
    fn viewport_program_declares_u_param_once() {
        let (body, mut fx) = filled_expr(FillMode::Sheet, Expr::Empty);
        let graded = generate_viewport(&body, Some(&fx));
        assert_eq!(
            graded.source.matches("uniform highp sampler3D u_param;").count(),
            1,
            "one declaration in the shared program"
        );
        assert!(
            graded.source.contains("float fs_grid(vec3 p) {"),
            "graded program:\n{}",
            graded.source
        );
        assert!(
            graded
                .grid
                .as_ref()
                .is_some_and(|g| g.dims == [7, 5, 4] && g.data.len() == 140),
            "the grid spec rides the emit"
        );
        fx.param.data = vec![0.45];
        let constant = generate_viewport(&body, Some(&fx));
        assert_eq!(
            constant.source.matches("uniform highp sampler3D u_param;").count(),
            0,
            "a single-node grid is a literal"
        );
        assert!(
            constant
                .source
                .contains("vec4 lat = vec4(abs(s.x) - 0.5 * 4.5e-1")
                && constant.grid.is_none(),
            "constant program:\n{}",
            constant.source
        );
    }
}
