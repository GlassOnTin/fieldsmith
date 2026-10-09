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

/// Read a 3MF file into named parts (one per `<object>` with a mesh; objects without a mesh
/// are skipped). The part name is the object's `name` attribute, or `mesh` when unnamed.
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

/// Parse the model XML: the `<model unit="…">` scale, then each `<object>`'s `<mesh>`.
fn parse_model(text: &str) -> Result<Vec<NamedPart>, String> {
    let mut s = Scanner { text, pos: 0 };
    let mut unit_scale = 1.0f64; // millimetre is the spec default
    let mut parts = Vec::new();
    while let Some(tag) = s.next_tag() {
        if tag.name == "model" {
            if let Some(u) = tag.attr("unit") {
                unit_scale = unit_scale_of(u)?;
            }
        } else if tag.name == "object" {
            let name = tag.attr("name").unwrap_or("mesh").to_string();
            if tag.self_closing {
                continue;
            }
            if let Some(mesh) = parse_object(&mut s, &name, unit_scale)? {
                parts.push(mesh);
            }
        }
    }
    if parts.is_empty() {
        return Err("3MF model has no object with a mesh".into());
    }
    Ok(parts)
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
fn parse_object(s: &mut Scanner, name: &str, unit_scale: f64) -> Result<Option<NamedPart>, String> {
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
    }))
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
