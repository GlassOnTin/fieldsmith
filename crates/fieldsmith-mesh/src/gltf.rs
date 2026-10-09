//! GLB (binary glTF 2.0) reading (goal M6, step 2).
//!
//! Policy for AI/external exports: everything the field model does not need (materials,
//! textures, cameras, unrecognised extensions) is ignored; anything that changes the
//! meaning of the geometry is rejected with a name (embedded or external buffers, quantised
//! attributes, skinned nodes, sparse accessors, non-TRIANGLE primitives). The JSON chunk is
//! parsed with `serde_json` into `Value`s and walked by hand — the schema is small and the
//! unknown-key-ignored policy is the default there. The chunk framing itself is read
//! manually so the reader stays one dependency deep.
//!
//! One node-tree walk bakes translation/rotation/scale (or matrix) into world coordinates;
//! a mesh instanced by several nodes appears once per placement inside its own part.

use crate::export::TriMesh;
use crate::NamedPart;
use serde_json::Value;

/// Column-major 4×4 matrix (`m[c]` is column `c`, matching glTF's `matrix` layout).
type Mat4 = [[f64; 4]; 4];

const IDENT: Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// A GLB buffer view: byte range inside the BIN chunk plus optional row stride.
struct Bv {
    off: usize,
    len: usize,
    stride: usize,
}

/// One accessor component decoder (componentType-specific) plus its byte width.
type LeReader = fn(&[u8], usize) -> f64;

/// Read a self-contained GLB. Returns one [`NamedPart`] per instanced mesh, in the order the
/// scene first reaches them. Every failure names the problem and the element that caused it.
pub fn read_glb(bytes: &[u8]) -> Result<Vec<NamedPart>, String> {
    if bytes.len() < 12 {
        return Err("GLB file is truncated (shorter than the 12-byte header)".into());
    }
    if &bytes[0..4] != b"glTF" {
        return Err("not a GLB file (the first bytes are not the 'glTF' magic)".into());
    }
    let version = le32(bytes, 4);
    if version != 2 {
        return Err(format!("GLB version {version}; only glTF 2.0 is supported"));
    }
    let total = le32(bytes, 8) as usize;
    if total > bytes.len() {
        return Err("GLB file is truncated (declared length exceeds the data present)".into());
    }
    let bytes = &bytes[..total];

    // ---- chunk framing -----------------------------------------------------------------------
    let mut json: Option<&str> = None;
    let mut bin: Option<&[u8]> = None;
    let mut off = 12usize;
    while off + 8 <= bytes.len() {
        let len = le32(bytes, off) as usize;
        let ty = &bytes[off + 4..off + 8];
        if len > bytes.len() - off - 8 {
            return Err("GLB chunk length overruns the file".into());
        }
        let payload = &bytes[off + 8..off + 8 + len];
        match ty {
            b"JSON" => {
                if json.is_some() {
                    return Err("GLB file has two JSON chunks; the spec allows one".into());
                }
                json = Some(
                    std::str::from_utf8(payload)
                        .map_err(|_| "GLB JSON chunk is not valid UTF-8".to_string())?,
                );
            }
            b"BIN\0" => {
                if bin.is_some() {
                    return Err("GLB file has more than one BIN chunk; the spec allows one".into());
                }
                bin = Some(payload);
            }
            _ => {} // unknown chunk types are ignored
        }
        off += 8 + len;
    }
    let json = json.ok_or("GLB file has no JSON chunk")?;
    let doc: Value =
        serde_json::from_str(json).map_err(|e| format!("GLB JSON does not parse: {e}"))?;
    if let Some(req) = doc.get("extensionsRequired").and_then(Value::as_array) {
        let names: Vec<String> = req
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect();
        if !names.is_empty() {
            return Err(format!(
                "GLB requires unsupported extension(s): {}; re-export the file without them",
                names.join(", ")
            ));
        }
    }

    // ---- buffer: exactly the BIN chunk ----------------------------------------------------------
    let empty = Vec::new();
    let bin = bin.unwrap_or(&[]);
    let buffers = doc
        .get("buffers")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let mut bin_at = None;
    for (i, b) in buffers.iter().enumerate() {
        if let Some(uri) = b.get("uri").and_then(Value::as_str) {
            if uri.starts_with("data:") {
                return Err(format!(
                    "buffer {i} embeds its data as a data URI; only self-contained GLB files are supported"
                ));
            }
            return Err(format!(
                "buffer {i} points at an external file ('{uri}'); only self-contained GLB files are supported"
            ));
        }
        if bin_at.is_some() {
            return Err(format!(
                "buffer {i} is a second uri-less buffer; a GLB carries exactly one (the BIN chunk)"
            ));
        }
        let blen = b.get("byteLength").and_then(Value::as_u64).unwrap_or(0) as usize;
        if bin.len() < blen {
            return Err(format!(
                "buffer {i} needs {blen} bytes but the GLB's BIN chunk holds {}",
                bin.len()
            ));
        }
        bin_at = Some(i);
    }

    // ---- buffer views --------------------------------------------------------------------------
    let views_json = doc
        .get("bufferViews")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let mut views = Vec::with_capacity(views_json.len());
    for (i, v) in views_json.iter().enumerate() {
        let bi = v.get("buffer").and_then(Value::as_u64).unwrap_or(u64::MAX) as usize;
        if Some(bi) != bin_at {
            return Err(format!(
                "buffer view {i} does not point at the GLB's BIN buffer"
            ));
        }
        let len = v.get("byteLength").and_then(Value::as_u64).unwrap_or(0) as usize;
        let off = v.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let stride = v.get("byteStride").and_then(Value::as_u64).unwrap_or(0) as usize;
        if off + len > bin.len() {
            return Err(format!("buffer view {i} extends past the BIN chunk"));
        }
        views.push(Bv { off, len, stride });
    }
    let accs = doc
        .get("accessors")
        .and_then(Value::as_array)
        .unwrap_or(&empty);

    // ---- node tree: instances in first-encounter order -------------------------------------------
    let meshes = doc
        .get("meshes")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let nodes = doc.get("nodes").and_then(Value::as_array).unwrap_or(&empty);
    let scenes = doc
        .get("scenes")
        .and_then(Value::as_array)
        .ok_or("GLB has no scenes array")?;
    let sc = doc.get("scene").and_then(Value::as_u64).unwrap_or(0) as usize;
    let scene = scenes.get(sc).ok_or_else(|| {
        format!(
            "GLB scene index {sc} does not exist ({} scenes)",
            scenes.len()
        )
    })?;

    let mut child_of = vec![false; nodes.len()];
    for n in nodes {
        if let Some(kids) = n.get("children").and_then(Value::as_array) {
            for c in kids {
                if let Some(ci) = c.as_u64().map(|u| u as usize) {
                    if ci < child_of.len() {
                        child_of[ci] = true;
                    }
                }
            }
        }
    }
    let roots: Vec<usize> = match scene.get("nodes").and_then(Value::as_array) {
        Some(list) if !list.is_empty() => {
            let mut out = Vec::new();
            for c in list {
                let ci = c
                    .as_u64()
                    .ok_or_else(|| format!("GLB scene root {c:?} is not a node index"))?
                    as usize;
                if ci >= nodes.len() {
                    return Err(format!(
                        "GLB scene root {ci} does not exist ({} nodes)",
                        nodes.len()
                    ));
                }
                out.push(ci);
            }
            out
        }
        _ => (0..nodes.len()).filter(|&i| !child_of[i]).collect(),
    };
    if roots.is_empty() {
        return Err("GLB scene contains no nodes".into());
    }

    struct Inst {
        world: Mat4,
        name_hint: String,
    }
    let mut order: Vec<usize> = Vec::new();
    let mut seen = vec![false; meshes.len()];
    let mut insts: Vec<Vec<Inst>> = (0..meshes.len()).map(|_| Vec::new()).collect();

    // Path-based cycle detection: a node shared by two parents is legal glTF (a DAG) and
    // produces two instances; appearing twice on one ancestry path is a cycle.
    fn visit(
        ni: usize,
        world: &Mat4,
        path: &mut Vec<usize>,
        nodes: &[Value],
        seen: &mut [bool],
        order: &mut Vec<usize>,
        insts: &mut [Vec<Inst>],
    ) -> Result<(), String> {
        if path.contains(&ni) {
            return Err("node tree contains a cycle".into());
        }
        path.push(ni);
        let node = &nodes[ni];
        if node.get("skin").is_some() {
            return Err(format!(
                "skinned meshes are not supported (node {ni} carries a skin)"
            ));
        }
        let local = node_matrix(node, ni)?;
        let world = mul(world, &local);
        if let Some(mi) = node.get("mesh").and_then(Value::as_u64) {
            let mi = mi as usize;
            if mi >= seen.len() {
                return Err(format!(
                    "node {ni} references mesh {mi} which does not exist"
                ));
            }
            if !seen[mi] {
                seen[mi] = true;
                order.push(mi);
            }
            let hint = node
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            insts[mi].push(Inst {
                world,
                name_hint: hint,
            });
        }
        if let Some(kids) = node.get("children").and_then(Value::as_array) {
            for c in kids {
                let ci = c
                    .as_u64()
                    .ok_or_else(|| format!("node {ni} child {c:?} is not an index"))?
                    as usize;
                if ci >= nodes.len() {
                    return Err(format!(
                        "node {ni} references child {ci} which does not exist"
                    ));
                }
                visit(ci, &world, path, nodes, seen, order, insts)?;
            }
        }
        path.pop();
        Ok(())
    }
    let mut path = Vec::new();
    for &r in &roots {
        visit(
            r, &IDENT, &mut path, nodes, &mut seen, &mut order, &mut insts,
        )?;
    }
    if order.is_empty() {
        return Err("the GLB scene contains no meshes".into());
    }

    // ---- geometry per mesh: primitives concatenated, once per instance ---------------------------
    let mut parts = Vec::with_capacity(order.len());
    for &mi in &order {
        let mesh = &meshes[mi];
        let prims = mesh
            .get("primitives")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("mesh {mi} has no primitives"))?;
        let mut verts: Vec<crate::V3> = Vec::new();
        let mut tris: Vec<[u32; 3]> = Vec::new();
        for (pi, prim) in prims.iter().enumerate() {
            let mode = prim.get("mode").and_then(Value::as_u64).unwrap_or(4);
            if mode != 4 {
                return Err(format!(
                    "mesh {mi} primitive {pi} has mode {mode}; only TRIANGLES (4) is supported"
                ));
            }
            let attrs = prim
                .get("attributes")
                .ok_or_else(|| format!("mesh {mi} primitive {pi} has no attributes"))?;
            let pai = attrs
                .get("POSITION")
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("mesh {mi} primitive {pi} has no POSITION attribute"))?
                as usize;
            let pos = read_accessor(accs, &views, bin, pai, 3, "VEC3")
                .map_err(|e| format!("mesh {mi} primitive {pi}: {e}"))?;
            let local: Vec<crate::V3> = pos.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
            let local_tris: Vec<[u32; 3]> = match prim.get("indices").and_then(Value::as_u64) {
                None => {
                    if !local.len().is_multiple_of(3) {
                        return Err(format!(
                            "mesh {mi} primitive {pi}: vertex count {} is not divisible by 3",
                            local.len()
                        ));
                    }
                    (0..local.len() / 3)
                        .map(|t| [(3 * t) as u32, (3 * t + 1) as u32, (3 * t + 2) as u32])
                        .collect()
                }
                Some(ii) => {
                    let ix = read_accessor(accs, &views, bin, ii as usize, 1, "SCALAR")
                        .map_err(|e| format!("mesh {mi} primitive {pi}: {e}"))?;
                    if !ix.len().is_multiple_of(3) {
                        return Err(format!(
                            "mesh {mi} primitive {pi}: index count {} is not divisible by 3",
                            ix.len()
                        ));
                    }
                    if ix.iter().any(|&v| v < 0.0 || v as usize >= local.len()) {
                        return Err(format!(
                            "mesh {mi} primitive {pi} indexes past its {} vertices",
                            local.len()
                        ));
                    }
                    ix.chunks_exact(3)
                        .map(|t| [t[0] as u32, t[1] as u32, t[2] as u32])
                        .collect()
                }
            };
            for inst in &insts[mi] {
                let base = verts.len() as u32;
                for p in &local {
                    verts.push(apply(&inst.world, *p));
                }
                for t in &local_tris {
                    tris.push([t[0] + base, t[1] + base, t[2] + base]);
                }
            }
        }
        if verts.is_empty() {
            return Err("the GLB scene contains no triangle geometry".into());
        }
        let name = mesh
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .or_else(|| {
                insts[mi]
                    .first()
                    .map(|i| i.name_hint.clone())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| format!("mesh{}", mi + 1));
        parts.push(NamedPart {
            name,
            mesh: TriMesh { verts, tris },
            material: None,
        });
    }
    Ok(parts)
}

// ---------------------------------------------------------------------------------------------
// numbers, matrices, accessors

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut c = [[0.0f64; 4]; 4];
    for j in 0..4 {
        for i in 0..4 {
            c[j][i] = (0..4).map(|k| a[k][i] * b[j][k]).sum();
        }
    }
    c
}

fn apply(m: &Mat4, p: crate::V3) -> crate::V3 {
    [
        m[0][0] * p[0] + m[1][0] * p[1] + m[2][0] * p[2] + m[3][0],
        m[0][1] * p[0] + m[1][1] * p[1] + m[2][1] * p[2] + m[3][1],
        m[0][2] * p[0] + m[1][2] * p[1] + m[2][2] * p[2] + m[3][2],
    ]
}

fn num(v: &Value, what: &str) -> Result<f64, String> {
    v.as_f64()
        .ok_or_else(|| format!("{what}: expected a number, found {v:?}"))
}

fn vec3(n: &Value, key: &str, ni: usize, default: [f64; 3]) -> Result<[f64; 3], String> {
    match n.get(key) {
        None => Ok(default),
        Some(v) => {
            let a = v
                .as_array()
                .ok_or_else(|| format!("node {ni} '{key}' must be an array of numbers"))?;
            if a.len() != 3 {
                return Err(format!(
                    "node {ni} '{key}' must have 3 entries, has {}",
                    a.len()
                ));
            }
            Ok([
                num(&a[0], &format!("node {ni} '{key}[0]'"))?,
                num(&a[1], &format!("node {ni} '{key}[1]'"))?,
                num(&a[2], &format!("node {ni} '{key}[2]'"))?,
            ])
        }
    }
}

/// The node's local matrix: `matrix` (column-major) when present, else TRS. A matrix whose
/// last row is not (0, 0, 0, 1) is projective and rejected — it would not keep a distance
/// field a distance field.
fn node_matrix(n: &Value, ni: usize) -> Result<Mat4, String> {
    if let Some(m) = n.get("matrix").and_then(Value::as_array) {
        if m.len() != 16 {
            return Err(format!(
                "node {ni} matrix must have 16 entries, has {}",
                m.len()
            ));
        }
        let mut mat: Mat4 = [[0.0; 4]; 4];
        for j in 0..4 {
            for i in 0..4 {
                mat[j][i] = num(&m[j * 4 + i], &format!("node {ni} matrix"))?;
            }
        }
        if mat[0][3] != 0.0 || mat[1][3] != 0.0 || mat[2][3] != 0.0 || mat[3][3] != 1.0 {
            return Err(format!(
                "node {ni} has a projective matrix; only affine node placements are supported"
            ));
        }
        return Ok(mat);
    }
    let t = vec3(n, "translation", ni, [0.0; 3])?;
    let s = vec3(n, "scale", ni, [1.0; 3])?;
    let q = match n.get("rotation") {
        None => [0.0, 0.0, 0.0, 1.0],
        Some(v) => {
            let a = v
                .as_array()
                .ok_or_else(|| format!("node {ni} 'rotation' must be an array of numbers"))?;
            if a.len() != 4 {
                return Err(format!(
                    "node {ni} 'rotation' must have 4 entries, has {}",
                    a.len()
                ));
            }
            [
                num(&a[0], &format!("node {ni} 'rotation[0]'"))?,
                num(&a[1], &format!("node {ni} 'rotation[1]'"))?,
                num(&a[2], &format!("node {ni} 'rotation[2]'"))?,
                num(&a[3], &format!("node {ni} 'rotation[3]'"))?,
            ]
        }
    };
    let n2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    let q = if n2 > 0.0 && (n2 - 1.0).abs() > 1e-8 {
        let inv = 1.0 / n2.sqrt();
        [q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv]
    } else {
        q
    };
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    // Rotation columns of the unit quaternion (right-handed, column-vector convention).
    let r: Mat4 = [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y + z * w),
            2.0 * (x * z - y * w),
            0.0,
        ],
        [
            2.0 * (x * y - z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z + x * w),
            0.0,
        ],
        [
            2.0 * (x * z + y * w),
            2.0 * (y * z - x * w),
            1.0 - 2.0 * (x * x + y * y),
            0.0,
        ],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut m = r;
    for c in 0..3 {
        for e in &mut m[c][0..3] {
            *e *= s[c];
        }
    }
    m[3] = [t[0], t[1], t[2], 1.0];
    Ok(m)
}

/// Read one accessor as flat f64 (`comps` = 3 for VEC3 positions, 1 for SCALAR indices).
/// Bounds are checked with the view's stride taken into account.
fn read_accessor(
    accs: &[Value],
    views: &[Bv],
    bin: &[u8],
    ai: usize,
    comps: usize,
    ty_name: &str,
) -> Result<Vec<f64>, String> {
    let acc = accs
        .get(ai)
        .ok_or_else(|| format!("accessor {ai} does not exist"))?;
    if acc
        .get("normalized")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(format!(
            "accessor {ai} is quantised (normalized); re-export the file without mesh quantisation"
        ));
    }
    if !matches!(acc.get("sparse"), None | Some(Value::Null)) {
        return Err(format!(
            "accessor {ai} uses a sparse layout; sparse accessors are not supported"
        ));
    }
    let ct = acc
        .get("componentType")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let (rd, width): (LeReader, usize) = match ct {
        5121 => (|b: &[u8], o: usize| b[o] as f64, 1),
        5122 => (
            |b: &[u8], o: usize| i16::from_le_bytes([b[o], b[o + 1]]) as f64,
            2,
        ),
        5123 => (
            |b: &[u8], o: usize| u16::from_le_bytes([b[o], b[o + 1]]) as f64,
            2,
        ),
        5125 => (
            |b: &[u8], o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as f64,
            4,
        ),
        5126 => (
            |b: &[u8], o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as f64,
            4,
        ),
        other => {
            return Err(format!(
                "accessor {ai} has componentType {other}; need one of 5121, 5122, 5123, 5125, 5126"
            ))
        }
    };
    let ty = acc.get("type").and_then(Value::as_str).unwrap_or("");
    if ty != ty_name {
        return Err(format!(
            "accessor {ai} has type \"{ty}\", expected \"{ty_name}\""
        ));
    }
    let count = acc
        .get("count")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("accessor {ai} has no count"))? as usize;
    let vi = acc
        .get("bufferView")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            format!("accessor {ai} fills from constants (no bufferView); not supported")
        })? as usize;
    let v = views
        .get(vi)
        .ok_or_else(|| format!("accessor {ai} refers to buffer view {vi} which does not exist"))?;
    if comps == 1 && v.stride != 0 {
        return Err(format!(
            "accessor {ai}: indices must be tightly packed (byteStride is set)"
        ));
    }
    let stride = if v.stride == 0 {
        width * comps
    } else {
        v.stride
    };
    let aoff = acc.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let needed = if count == 0 {
        0
    } else {
        (count - 1) * stride + comps * width
    };
    if aoff + needed > v.len {
        return Err(format!(
            "accessor {ai} reads {} bytes but its buffer view holds {} (overrun)",
            aoff + needed,
            v.len
        ));
    }
    let mut out = Vec::with_capacity(count * comps);
    for e in 0..count {
        for c in 0..comps {
            let at = v.off + aoff + e * stride + c * width;
            if at + width > bin.len() {
                return Err(format!("accessor {ai} reads past the end of the BIN chunk"));
            }
            out.push(rd(bin, at));
        }
    }
    Ok(out)
}
