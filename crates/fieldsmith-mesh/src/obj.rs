//! Wavefront OBJ reading (goal M6, step 2).
//!
//! Faces belong to the object/group (`o`/`g`) they follow; text before the first such line
//! is an unnamed part. Faces referencing global vertex indices (`v` lines) are fanned into
//! triangles per part, and each part is materialised with its own first-use vertex remap, so
//! the parts of one file share no vertices. Everything else in the file (`vt`, `vn`, `s`,
//! `usemtl`, `mtllib`, `l`, comments, unknown keys) is skipped.

use crate::export::TriMesh;
use crate::NamedPart;

/// Read OBJ text in world coordinates. Every failure names the problem and its line (1-based).
pub fn read_obj(text: &str) -> Result<Vec<NamedPart>, String> {
    struct Part {
        name: String,
        tris: Vec<[u32; 3]>,
    }
    let mut gv: Vec<crate::V3> = Vec::new();
    let mut parts: Vec<Part> = Vec::new();
    let mut cur: Option<usize> = None;

    for (li, line) in text.lines().enumerate() {
        let n = li + 1;
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let mut p = [0.0f64; 3];
                for slot in &mut p {
                    let tok = it
                        .next()
                        .ok_or_else(|| format!("line {n}: 'v' line needs x y z"))?;
                    *slot = tok
                        .parse::<f64>()
                        .map_err(|_| format!("line {n}: position '{tok}' is not a number"))?;
                }
                if let Some(w) = it.next() {
                    let w: f64 = w
                        .parse()
                        .map_err(|_| format!("line {n}: weight '{w}' is not a number"))?;
                    if (w - 1.0).abs() > 1e-12 {
                        return Err(format!(
                            "line {n}: vertex weight w = {w}; rational (projective) OBJ vertices are not supported"
                        ));
                    }
                }
                gv.push(p);
            }
            Some("f") => {
                let mut poly: Vec<u32> = Vec::new();
                for tok in it {
                    let first = tok.split('/').next().unwrap_or("");
                    let iv: i64 = first.parse().map_err(|_| {
                        format!("line {n}: face reference '{tok}' is not a vertex index")
                    })?;
                    let idx = if iv > 0 {
                        (iv - 1) as usize
                    } else if iv < 0 {
                        (gv.len() as i64 + iv) as usize
                    } else {
                        return Err(format!(
                            "line {n}: face reference '{tok}' is 0; OBJ indices start at 1"
                        ));
                    };
                    if gv.is_empty() || idx >= gv.len() {
                        return Err(format!(
                            "line {n}: face references vertex {iv} but only {} exist so far",
                            gv.len()
                        ));
                    }
                    poly.push(idx as u32);
                }
                if poly.len() < 3 {
                    return Err(format!(
                        "line {n}: face has {} vertices; 3 are needed",
                        poly.len()
                    ));
                }
                let c = match cur {
                    Some(c) => c,
                    None => {
                        parts.push(Part {
                            name: String::new(),
                            tris: Vec::new(),
                        });
                        cur = Some(parts.len() - 1);
                        parts.len() - 1
                    }
                };
                for i in 1..poly.len() - 1 {
                    parts[c].tris.push([poly[0], poly[i], poly[i + 1]]);
                }
            }
            Some("o") | Some("g") => {
                let name = line
                    .trim()
                    .split_once(char::is_whitespace)
                    .map(|(_, rest)| rest.trim().to_string())
                    .unwrap_or_default();
                parts.push(Part {
                    name,
                    tris: Vec::new(),
                });
                cur = Some(parts.len() - 1);
            }
            _ => {} // vt, vn, s, usemtl, mtllib, l, comments, unknown directives: ignored
        }
    }

    let mut out = Vec::new();
    for p in parts {
        if p.tris.is_empty() {
            continue; // an object/group header with no faces carries no geometry
        }
        let mut remap: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        let mut verts: Vec<crate::V3> = Vec::new();
        let mut tris = Vec::with_capacity(p.tris.len());
        for t in &p.tris {
            tris.push(t.map(|vi| {
                let next = verts.len() as u32;
                *remap.entry(vi).or_insert_with(|| {
                    verts.push(gv[vi as usize]);
                    next
                })
            }));
        }
        out.push(NamedPart {
            name: p.name,
            mesh: TriMesh { verts, tris },
        });
    }
    if out.is_empty() {
        return Err("the OBJ file contains no faces".into());
    }
    Ok(out)
}
