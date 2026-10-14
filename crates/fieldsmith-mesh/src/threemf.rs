//! 3MF reader (goal M7, step 6): read back the 3D Manufacturing Format our own
//! [`crate::export::TriMesh::write_3mf`] writes, plus the common single-object files other
//! tools emit.
//!
//! A 3MF file is a zip archive. The model lives at `3D/3dmodel.model` (the path is fixed by
//! the spec's core namespace; the `_rels/.rels` relationship points at it, and our writer
//! uses exactly this layout). The model XML carries `<object>` elements, each with a
//! `<mesh>` of `<vertices>` and `<triangles>`. Units come from the `<model unit="…">`
//! attribute; the spec's units are millimetre (default), micron, centimetre, metre, inch and
//! foot — we read millimetre, centimetre, metre and inch (micron and foot are refused by
//! name, not silently mis-scaled).
//!
//! The parser is a small hand-rolled XML scanner, not a DOM: it walks tags in order and
//! pulls the attributes it needs. That is enough for the model file's flat, repetitive
//! structure (thousands of `<vertex>`/`<triangle>` elements), and it keeps the crate free of
//! an XML dependency. It is deliberately strict about what it accepts: mismatched tags,
//! missing required attributes and out-of-range triangle indices are named errors, not
//! silent skips.

use crate::export::TriMesh;
use crate::zip::read_zip;
use crate::{NamedPart, V3};
use std::collections::HashMap;

/// Read a 3MF file into named parts (one per `<build>` item, so an instanced object is
/// expanded to one part per placement). The part name is the object's `name` attribute, or
/// `mesh` when unnamed; the part's material is the object's `pid`/`pindex` resolved through
/// the model's `<basematerials>` groups, or `None` when the file records none.
pub fn read_3mf(bytes: &[u8]) -> Result<Vec<NamedPart>, String> {
    let entries = read_zip(bytes)?;
    let model = entries
        .iter()
        .find(|e| e.name == "3D/3dmodel.model" || e.name == "/3D/3dmodel.model")
        .ok_or_else(|| "3MF archive has no 3D/3dmodel.model entry".to_string())?;
    let text = std::str::from_utf8(&model.data)
        .map_err(|_| "3D/3dmodel.model is not valid UTF-8".to_string())?;
    parse_model(text)
}

/// Parse the model XML: the `<model unit="…">` scale, the `<basematerials>` groups, then
/// each `<object>`'s `<mesh>`.
fn parse_model(text: &str) -> Result<Vec<NamedPart>, String> {
    // A first pass collects the `<basematerials>` groups so an object's `pid` resolves even
    // when its group is declared after the object in the resources. `pid` values name those
    // groups; the object's `pindex` picks one base name (0-based).
    let mats = collect_material_groups(text)?;
    let mut s = Scanner { text, pos: 0 };
    let mut unit_scale = 1.0f64; // millimetre is the spec default
                                 // Resource objects by id, in file order; `None` is an object without a `<mesh>`
                                 // (e.g. a component container, whose subtree is walked through).
    let mut objs: Vec<(u64, Option<NamedPart>)> = Vec::new();
    let mut saw_build = false;
    let mut parts = Vec::new();
    while let Some(tag) = s.next_tag() {
        if tag.name == "model" {
            if let Some(u) = tag.attr("unit") {
                unit_scale = unit_scale_of(u)?;
            }
        } else if tag.name == "object" && !tag.closing {
            let name = tag.attr("name").unwrap_or("mesh").to_string();
            // The id is parsed and checked for reuse even on a self-closing object: a
            // re-used resource id is a spec violation regardless of what follows.
            let id = match tag.attr("id") {
                Some(i) => i.parse::<u64>().map_err(|_| {
                    format!(
                        "3MF object with name \"{name}\" has id \"{i}\", which is not an integer"
                    )
                })?,
                None => {
                    return Err(format!("3MF object \"{name}\" has no id attribute"));
                }
            };
            if objs.iter().any(|(k, _)| *k == id) {
                return Err(format!("3MF object id '{id}' is used more than once"));
            }
            if tag.self_closing {
                objs.push((id, None));
                continue;
            }
            let material = match (tag.attr("pid"), tag.attr("pindex")) {
                (None, None) => None,
                (pid, pindex) => {
                    let pid = material_group(&tag, pid)?;
                    let idx = (pindex.unwrap_or("0"))
                        .parse::<usize>()
                        .map_err(|_| format!("3MF object '{name}' pindex is not an integer"))?;
                    let group = mats
                        .get(&pid)
                        .ok_or_else(|| format!("3MF object '{name}' has pid=\"{pid}\" but the model defines no basematerials group with id {pid}"))?;
                    let label = group.get(idx).filter(|l| !l.is_empty()).ok_or_else(|| {
                        format!(
                            "3MF object '{name}' has pindex=\"{idx}\" but basematerials {pid} has {} base materials",
                            group.len()
                        )
                    })?;
                    Some(label.clone())
                }
            };
            let mesh = parse_object(&mut s, &name, unit_scale, material)?;
            objs.push((id, mesh));
        } else if tag.name == "build" && !tag.closing && !tag.self_closing {
            saw_build = true;
            // Each `<item>` places a resource object in build space; the item is the
            // printed part, so one part per item, with the item's transform (12 numbers —
            // the rows of a 4×3 affine matrix, translation last, spec §3.3) baked into the
            // vertices as the other readers deliver source coordinates.
            loop {
                let Some(it) = s.next_tag() else {
                    return Err("3MF <build> is not closed".into());
                };
                if it.closing && it.name == "build" {
                    break;
                }
                if it.name != "item" || it.closing {
                    if !it.self_closing && !it.closing {
                        skip_subtree(&mut s);
                    }
                    continue;
                }
                let oid = match it.attr("objectid") {
                    Some(o) => o.parse::<u64>().map_err(|_| {
                        format!("3MF build item has objectid \"{o}\", which is not an integer")
                    })?,
                    None => return Err("3MF build item has no objectid attribute".into()),
                };
                // A build item may reference an object id the model never defined, or a
                // component container object that has no mesh of its own; both are refused
                // by name rather than silently dropped (that would change the part set).
                let part = objs
                    .iter()
                    .find(|(k, _)| *k == oid)
                    .and_then(|(_, p)| p.as_ref())
                    .ok_or_else(|| {
                        match objs.iter().any(|(k, _)| *k == oid) {
                            true => format!(
                                "3MF build item references object id '{oid}' which has no mesh (component objects are not supported)"
                            ),
                            false => format!(
                                "3MF build item references object id '{oid}' which the model does not define"
                            ),
                        }
                    })?;
                parts.push(placed_item(&it, part, unit_scale)?);
            }
        }
    }
    // The spec requires <build>; writers that omit it still get their resource meshes (as
    // before), so a missing build is not an error. A model with a build ends up with
    // exactly what the build places.
    if !saw_build {
        parts = objs.into_iter().filter_map(|(_, p)| p).collect();
    }
    if parts.is_empty() {
        return Err("3MF model has no object with a mesh".into());
    }
    Ok(parts)
}

/// One build item placed: the object's mesh with the item's `transform` baked into the
/// vertices. The matrix's linear part is applied as stored (the vertices already carry the
/// model-unit scale), and the translation is scaled by the model unit. A negative-
/// determinant transform mirrors the shape, so the triangle winding is reversed to keep
/// the volume's sign (spec §3.3). Without a transform the object is returned unchanged.
/// The part keeps the object's name, even under instancing; `import_mesh` de-duplicates
/// ids when one object is built more than once.
fn placed_item(tag: &Tag, part: &NamedPart, unit_scale: f64) -> Result<NamedPart, String> {
    let Some(t) = tag.attr("transform") else {
        return Ok(part.clone());
    };
    let words: Vec<&str> = t.split_whitespace().collect();
    if words.len() != 12 {
        return Err(format!(
            "3MF build item transform has {} numbers, expected 12",
            words.len()
        ));
    }
    let mut m = [0.0f64; 12];
    for (i, w) in words.iter().enumerate() {
        m[i] = w
            .parse::<f64>()
            .map_err(|_| format!("3MF build item transform value '{w}' is not a number"))?;
    }
    let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
        + m[2] * (m[3] * m[7] - m[4] * m[6]);
    let mut verts = Vec::with_capacity(part.mesh.verts.len());
    for v in &part.mesh.verts {
        verts.push([
            m[0] * v[0] + m[1] * v[1] + m[2] * v[2] + m[9] * unit_scale,
            m[3] * v[0] + m[4] * v[1] + m[5] * v[2] + m[10] * unit_scale,
            m[6] * v[0] + m[7] * v[1] + m[8] * v[2] + m[11] * unit_scale,
        ]);
    }
    let tris: Vec<[u32; 3]> = if det < 0.0 {
        part.mesh.tris.iter().map(|t| [t[0], t[2], t[1]]).collect()
    } else {
        part.mesh.tris.clone()
    };
    Ok(NamedPart {
        name: part.name.clone(),
        mesh: TriMesh { verts, tris },
        material: part.material.clone(),
    })
}

/// Walk the model XML once and collect every `<basematerials>` group by id. Any other
/// opening element's whole subtree is skipped, so the pass costs tag scans, not a second
/// read of vertex and triangle data.
fn collect_material_groups(text: &str) -> Result<HashMap<u64, Vec<String>>, String> {
    let mut s = Scanner { text, pos: 0 };
    let mut mats: HashMap<u64, Vec<String>> = HashMap::new();
    while let Some(tag) = s.next_tag() {
        if tag.closing {
            continue;
        }
        match tag.name.as_str() {
            // Objects carry the vertex/triangle bulk and no legal basematerials of their
            // own: skip their whole subtree. Everything else (<model>, <resources>,
            // <build>, <metadata>) is scanned through.
            "object" => {
                if !tag.self_closing {
                    skip_subtree(&mut s);
                }
            }
            "basematerials" => {
                let id = tag
                    .attr("id")
                    .ok_or_else(|| "3MF <basematerials> has no id attribute".to_string())?
                    .parse::<u64>()
                    .map_err(|_| "3MF <basematerials> id is not an integer".to_string())?;
                if mats.contains_key(&id) {
                    return Err(format!(
                        "3MF basematerials id '{id}' is used more than once"
                    ));
                }
                let mut base = Vec::new();
                if !tag.self_closing {
                    loop {
                        match s.next_tag() {
                            None => return Err("3MF <basematerials> is not closed".into()),
                            Some(t) if t.closing && t.name == "basematerials" => break,
                            Some(t) if t.closing => {
                                return Err(format!(
                                    "3MF <basematerials>: unexpected closing tag </{}>",
                                    t.name
                                ))
                            }
                            Some(t) if t.name == "base" && !t.closing => {
                                let name = req_str(&t, "name", "3MF")?;
                                base.push(name);
                            }
                            Some(_) => {}
                        }
                    }
                }
                mats.insert(id, base);
            }
            _ => {}
        }
    }
    Ok(mats)
}

/// Consume tags until the subtree opened by the current element is closed. Comments and
/// declarations are handled by the scanner; nesting counted from this element's open tag.
fn skip_subtree(s: &mut Scanner) {
    let mut depth = 1usize;
    while depth > 0 {
        match s.next_tag() {
            None => return, // truncated input; the object pass reports a named error
            Some(t) => {
                if t.closing {
                    depth -= 1;
                } else if !t.self_closing {
                    depth += 1;
                }
            }
        }
    }
}

/// A material group id from an object's `pid`; `None` means no material reference. Objects
/// that carry a `pindex` without a `pid` are refused: the spec requires both.
fn material_group(tag: &Tag, pid: Option<&str>) -> Result<u64, String> {
    let pid_v = pid.ok_or_else(|| {
        format!(
            "3MF object has pindex=\"{}\" but no pid attribute",
            tag.attr("pindex").unwrap_or("")
        )
    })?;
    pid_v
        .parse::<u64>()
        .map_err(|_| format!("3MF object pid=\"{pid_v}\" is not an integer"))
}

/// Scale factor from the 3MF `unit` attribute to millimetres.
fn unit_scale_of(u: &str) -> Result<f64, String> {
    match u {
        "millimeter" | "millimetre" => Ok(1.0),
        "centimeter" | "centimetre" => Ok(10.0),
        "meter" | "metre" => Ok(1000.0),
        "inch" => Ok(25.4),
        other => Err(format!(
            "3MF unit '{other}' is not one of millimeter, centimeter, meter, inch"
        )),
    }
}

/// Parse one `<object>`: scan to its `<mesh>` (skipping components/other children), then
/// read vertices and triangles. Returns `None` when the object has no `<mesh>`.
fn parse_object(
    s: &mut Scanner,
    name: &str,
    unit_scale: f64,
    material: Option<String>,
) -> Result<Option<NamedPart>, String> {
    let mut verts: Vec<V3> = Vec::new();
    let mut tris: Vec<[u32; 3]> = Vec::new();
    let mut in_mesh = false;
    let mut depth = 1usize; // we are inside <object>
    while depth > 0 {
        let Some(tag) = s.next_tag() else {
            return Err(format!(
                "3MF object '{name}' is not closed (missing </object>)"
            ));
        };
        if tag.closing {
            depth -= 1;
            if depth == 0 {
                break;
            }
            if in_mesh && tag.name == "mesh" {
                in_mesh = false;
            }
            continue;
        }
        if tag.self_closing {
            if in_mesh && tag.name == "vertex" {
                let x = req_f64(&tag, "x", name)?;
                let y = req_f64(&tag, "y", name)?;
                let z = req_f64(&tag, "z", name)?;
                verts.push([x * unit_scale, y * unit_scale, z * unit_scale]);
            }
            continue;
        }
        match tag.name.as_str() {
            "mesh" if !in_mesh => {
                in_mesh = true;
                depth += 1;
            }
            "vertices" if in_mesh => {
                depth += 1;
                // Vertices are all self-closing <vertex/> elements; the scanner reads them
                // as tags above. Nothing to do here beyond entering the element.
            }
            "triangles" if in_mesh => {
                depth += 1;
                while let Some(t) = s.next_tag() {
                    if t.closing {
                        if t.name == "triangles" {
                            depth -= 1;
                            break;
                        }
                        return Err(format!(
                            "3MF object '{name}': unexpected closing tag </{}> inside <triangles>",
                            t.name
                        ));
                    }
                    if t.name != "triangle" {
                        return Err(format!(
                            "3MF object '{name}': expected <triangle>, found <{}>",
                            t.name
                        ));
                    }
                    // Per-triangle material (a `pid` on a <triangle>) is refused rather
                    // than dropped: a mesh whose faces carry different materials would silently
                    // lose that labelling on import.
                    if t.attr("pid").is_some() {
                        return Err(format!(
                            "3MF object '{name}': per-triangle material (pid on a triangle) is not supported"
                        ));
                    }
                    let v1 = req_u32(&t, "v1", name)?;
                    let v2 = req_u32(&t, "v2", name)?;
                    let v3 = req_u32(&t, "v3", name)?;
                    for &v in &[v1, v2, v3] {
                        if v as usize >= verts.len() {
                            return Err(format!(
                                "3MF object '{name}': triangle index {v} is out of range ({} vertices read so far)",
                                verts.len()
                            ));
                        }
                    }
                    tris.push([v1, v2, v3]);
                    if !t.self_closing {
                        // consume the closing </triangle>
                        match s.next_tag() {
                            Some(c) if c.closing && c.name == "triangle" => {}
                            _ => {
                                return Err(format!(
                                    "3MF object '{name}': <triangle> is not closed"
                                ))
                            }
                        }
                    }
                }
            }
            "object" => {
                // nested objects are not a thing in the core profile; skip the subtree
                depth += 1;
            }
            _ => {
                depth += 1;
            }
        }
    }
    if !in_mesh && verts.is_empty() && tris.is_empty() {
        return Ok(None); // no <mesh> in this object
    }
    if verts.is_empty() {
        return Err(format!("3MF object '{name}' has a mesh with no vertices"));
    }
    if tris.is_empty() {
        return Err(format!("3MF object '{name}' has a mesh with no triangles"));
    }
    Ok(Some(NamedPart {
        name: name.to_string(),
        mesh: TriMesh { verts, tris },
        material,
    }))
}

fn req_str(tag: &Tag, attr: &str, what: &str) -> Result<String, String> {
    match tag.attr(attr) {
        Some(v) => Ok(v.to_string()),
        None => Err(format!(
            "{what} <{}> is missing its {attr} attribute",
            tag.name
        )),
    }
}

fn req_f64(tag: &Tag, attr: &str, obj: &str) -> Result<f64, String> {
    match tag.attr(attr) {
        Some(v) => v.parse::<f64>().map_err(|_| {
            format!(
                "3MF object '{obj}': <{}> attribute {attr}=\"{v}\" is not a number",
                tag.name
            )
        }),
        None => Err(format!(
            "3MF object '{obj}': <{}> is missing its {attr} attribute",
            tag.name
        )),
    }
}

fn req_u32(tag: &Tag, attr: &str, obj: &str) -> Result<u32, String> {
    match tag.attr(attr) {
        Some(v) => v.parse::<u32>().map_err(|_| {
            format!(
                "3MF object '{obj}': <{}> attribute {attr}=\"{v}\" is not an index",
                tag.name
            )
        }),
        None => Err(format!(
            "3MF object '{obj}': <{}> is missing its {attr} attribute",
            tag.name
        )),
    }
}

// ---------------------------------------------------------------------------------------------
// Minimal XML tag scanner

struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
    closing: bool,
    self_closing: bool,
}

impl Tag {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

struct Scanner<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Scanner<'a> {
    /// The next tag (opening, closing or self-closing), skipping text and comments.
    /// Returns `None` at end of input.
    fn next_tag(&mut self) -> Option<Tag> {
        let bytes = self.text.as_bytes();
        while self.pos < bytes.len() {
            if bytes[self.pos] != b'<' {
                self.pos += 1;
                continue;
            }
            // Comments and declarations (<?xml …?>, <!DOCTYPE …>) are skipped.
            if self.text[self.pos..].starts_with("<!--") {
                let e = self.text[self.pos..].find("-->")?;
                self.pos += e + 3;
                continue;
            }
            if self.text[self.pos..].starts_with("<?") {
                let e = self.text[self.pos..].find("?>")?;
                self.pos += e + 2;
                continue;
            }
            if self.text[self.pos..].starts_with("<!") {
                let e = self.text[self.pos..].find('>')?;
                self.pos += e + 1;
                continue;
            }
            // A real tag: find its end, honouring quoted attribute values.
            let start = self.pos + 1;
            let mut i = start;
            let mut quote = false;
            while i < bytes.len() {
                let c = bytes[i];
                if quote {
                    if c == b'"' {
                        quote = false;
                    }
                } else if c == b'"' {
                    quote = true;
                } else if c == b'>' {
                    break;
                }
                i += 1;
            }
            if i >= bytes.len() {
                return None;
            }
            let inner = &self.text[start..i];
            self.pos = i + 1;
            return Some(parse_tag(inner));
        }
        None
    }
}

/// Parse the inside of a tag (`name attr="value" …`, `/name`, `name/`).
fn parse_tag(inner: &str) -> Tag {
    let inner = inner.trim();
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, inner),
    };
    let (self_closing, inner) = match inner.strip_suffix('/') {
        Some(rest) => (true, rest.trim_end()),
        None => (false, inner),
    };
    // The name is the leading run of non-whitespace, non-'=' characters.
    let name_end = inner
        .find(|c: char| c.is_whitespace() || c == '=' || c == '/')
        .unwrap_or(inner.len());
    let name = inner[..name_end].to_string();
    let rest = &inner[name_end..];
    // Attributes: key="value" pairs, whitespace-separated. Values may contain spaces
    // (quoted); keys may not.
    let mut attrs = Vec::new();
    let b = rest.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let key_start = i;
        while i < b.len() && b[i] != b'=' && !(b[i] as char).is_whitespace() {
            i += 1;
        }
        let key = rest[key_start..i].to_string();
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] != b'=' {
            // A bare attribute (no value): record it as an empty string.
            attrs.push((key, String::new()));
            continue;
        }
        i += 1; // skip '='
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        if i < b.len() && b[i] == b'"' {
            i += 1;
            let val_start = i;
            while i < b.len() && b[i] != b'"' {
                i += 1;
            }
            let val = unescape(&rest[val_start..i]);
            if i < b.len() {
                i += 1; // skip the closing quote
            }
            attrs.push((key, val));
        } else {
            // Unquoted value (not spec-valid, but tolerate it).
            let val_start = i;
            while i < b.len() && !(b[i] as char).is_whitespace() {
                i += 1;
            }
            attrs.push((key, rest[val_start..i].to_string()));
        }
    }
    Tag {
        name,
        attrs,
        closing,
        self_closing,
    }
}

/// XML-unescape the five built-in entities.
fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}
