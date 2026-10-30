//! Direct implicit-field 2D contour slicing and 3MF Slice Extension export/import (M27, V49).
//!
//! Evaluates `f(x, y, z) = 0` on a sequence of Z planes using 2D marching squares with
//! along-edge root polishing, stitches the cell segments into closed oriented polygons
//! (outer contours counter-clockwise, holes clockwise), and writes/reads `<s:slicestack>`
//! packages conforming to the 3MF Slice Extension specification.

use crate::zip::{read_zip, stream_entry, write_central, ZipStream};
use crate::V3;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fmt::Write as _;

/// One 2D cross-section of an implicit solid at height `z` (mm).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SliceLayer {
    /// Plane height in model coordinates (mm).
    pub z: f64,
    /// Closed polygon loops `[x, y]` (mm): outer contours wind CCW (positive signed area),
    /// interior holes wind CW (negative signed area). Every loop has `>= 3` distinct vertices
    /// and its last vertex connects back to `loop[0]`.
    pub polygons: Vec<Vec<[f64; 2]>>,
    /// Number of unclosed segment chains encountered during stitching (0 for any slice
    /// strictly inside the padded bounding box).
    pub open_chains: usize,
}

/// A stack of 2D cross-section layers spanning `z_bottom .. z_top` at uniform `layer_h`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SliceStack {
    /// Bottom Z of the slice stack in model coordinates (mm).
    pub z_bottom: f64,
    /// Layer thickness (mm).
    pub layer_h: f64,
    /// Sampling grid spacing in X/Y used to extract contours (mm).
    pub h: f64,
    /// Minimum corner `[x_min, y_min, z_min]` of the sliced bounding box (mm).
    pub origin: V3,
    /// Ordered layers (ascending `z`).
    pub layers: Vec<SliceLayer>,
}

impl SliceLayer {
    /// Net 2D cross-sectional area (mm²): outer CCW loops contribute positive area,
    /// inner CW hole loops contribute negative area.
    pub fn area(&self) -> f64 {
        self.polygons.iter().map(|p| polygon_signed_area(p)).sum()
    }

    /// Total number of polygon vertices across all closed loops in this layer.
    pub fn vertex_count(&self) -> usize {
        self.polygons.iter().map(|p| p.len()).sum()
    }

    /// Packed line segments `[x0, y0, x1, y1, ...]` (mm) for viewport overlay rendering.
    pub fn packed_segments(&self) -> Vec<f64> {
        let total_edges: usize = self.polygons.iter().map(|p| p.len()).sum();
        let mut out = Vec::with_capacity(total_edges * 4);
        for poly in &self.polygons {
            let n = poly.len();
            if n < 2 {
                continue;
            }
            for i in 0..n {
                let a = poly[i];
                let b = poly[(i + 1) % n];
                out.extend_from_slice(&[a[0], a[1], b[0], b[1]]);
            }
        }
        out
    }
}

impl SliceStack {
    /// Total integrated volume across all layers (`sum(layer.area() * layer_h)`, mm³).
    pub fn volume(&self) -> f64 {
        self.layers.iter().map(|l| l.area() * self.layer_h).sum()
    }

    /// Total closed polygon count across all layers.
    pub fn polygon_count(&self) -> usize {
        self.layers.iter().map(|l| l.polygons.len()).sum()
    }

    /// Total open chains across all layers (must be 0 for a valid slice stack).
    pub fn open_chains(&self) -> usize {
        self.layers.iter().map(|l| l.open_chains).sum()
    }
}

/// Signed area of a closed 2D polygon `[x, y]` via the shoelace formula (mm²).
pub fn polygon_signed_area(poly: &[[f64; 2]]) -> f64 {
    let n = poly.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        sum += a[0] * b[1] - b[0] * a[1];
    }
    0.5 * sum
}

/// Root-polish a zero crossing of `f` along the 2D segment `a -> b` at height `z`.
fn polish_crossing_2d(
    f: &impl Fn(V3) -> f64,
    a: [f64; 2],
    b: [f64; 2],
    z: f64,
    fa: f64,
    fb: f64,
) -> [f64; 2] {
    let (mut t0, mut t1, mut f0, mut f1) = (0.0f64, 1.0f64, fa, fb);
    let mut side = 0i8;
    let mut t = (f0 / (f0 - f1)).clamp(1e-4, 1.0 - 1e-4);
    for _ in 0..16 {
        t = (t0 * f1 - t1 * f0) / (f1 - f0);
        if !(t > t0 && t < t1) {
            t = 0.5 * (t0 + t1);
        }
        let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, z];
        let ft = f(p);
        if ft == 0.0 || (t1 - t0) < 1e-9 {
            break;
        }
        if (ft < 0.0) == (f0 < 0.0) {
            t0 = t;
            f0 = ft;
            if side == -1 {
                f1 *= 0.5;
            }
            side = -1;
        } else {
            t1 = t;
            f1 = ft;
            if side == 1 {
                f0 *= 0.5;
            }
            side = 1;
        }
    }
    let t = t.clamp(1e-6, 1.0 - 1e-6);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// Extract closed 2D iso-contours `f(x, y, z) = 0` at height `z` over `[lo, hi]` with grid
/// spacing `h` (mm). The X/Y domain is padded by `1.5 * h` outside `[lo, hi]` so contours
/// never terminate on the sampling boundary.
pub fn slice_field(f: impl Fn(V3) -> f64, lo: V3, hi: V3, z: f64, h: f64) -> SliceLayer {
    let h = h.max(1e-4);
    let pad = 1.5 * h;
    let x0 = lo[0] - pad;
    let y0 = lo[1] - pad;
    let x1 = hi[0] + pad;
    let y1 = hi[1] + pad;
    let nx = ((x1 - x0) / h).ceil().max(2.0) as usize;
    let ny = ((y1 - y0) / h).ceil().max(2.0) as usize;
    let hx = (x1 - x0) / nx as f64;
    let hy = (y1 - y0) / ny as f64;

    let stride = nx + 1;
    let mut vals = vec![0.0f64; (nx + 1) * (ny + 1)];
    let eps = 1e-12 * h;
    for j in 0..=ny {
        let y = y0 + j as f64 * hy;
        for i in 0..=nx {
            let x = x0 + i as f64 * hx;
            let mut v = if i == 0 || i == nx || j == 0 || j == ny {
                // Guarantee the outer padded ring is strictly outside so every interior sign
                // change closes inside the grid.
                f([x, y, z]).max(eps)
            } else {
                f([x, y, z])
            };
            if v.abs() < eps {
                v = eps;
            }
            vals[j * stride + i] = v;
        }
    }

    // Unique vertex per grid edge that changes sign:
    // Horizontal edges: (i, j) -> (i+1, j), i in 0..nx, j in 0..=ny
    // Vertical edges:   (i, j) -> (i, j+1), i in 0..=nx, j in 0..ny
    let mut verts: Vec<[f64; 2]> = Vec::new();
    let mut h_edge = vec![u32::MAX; nx * (ny + 1)];
    let mut v_edge = vec![u32::MAX; (nx + 1) * ny];

    for j in 0..=ny {
        let y = y0 + j as f64 * hy;
        for i in 0..nx {
            let fa = vals[j * stride + i];
            let fb = vals[j * stride + i + 1];
            if (fa < 0.0) != (fb < 0.0) {
                let x = x0 + i as f64 * hx;
                let id = verts.len() as u32;
                verts.push(polish_crossing_2d(&f, [x, y], [x + hx, y], z, fa, fb));
                h_edge[j * nx + i] = id;
            }
        }
    }
    for j in 0..ny {
        let y = y0 + j as f64 * hy;
        for i in 0..=nx {
            let fa = vals[j * stride + i];
            let fb = vals[(j + 1) * stride + i];
            if (fa < 0.0) != (fb < 0.0) {
                let x = x0 + i as f64 * hx;
                let id = verts.len() as u32;
                verts.push(polish_crossing_2d(&f, [x, y], [x, y + hy], z, fa, fb));
                v_edge[j * stride + i] = id;
            }
        }
    }

    // Every crossing vertex has at most 1 outgoing directed edge (solid < 0 on the left).
    let mut next_vert = vec![u32::MAX; verts.len()];
    for j in 0..ny {
        for i in 0..nx {
            let v0 = vals[j * stride + i];
            let v1 = vals[j * stride + i + 1];
            let v2 = vals[(j + 1) * stride + i + 1];
            let v3 = vals[(j + 1) * stride + i];
            let mask = ((v0 < 0.0) as u8)
                | (((v1 < 0.0) as u8) << 1)
                | (((v2 < 0.0) as u8) << 2)
                | (((v3 < 0.0) as u8) << 3);
            if mask == 0 || mask == 15 {
                continue;
            }
            let e0 = h_edge[j * nx + i];
            let e1 = v_edge[j * stride + i + 1];
            let e2 = h_edge[(j + 1) * nx + i];
            let e3 = v_edge[j * stride + i];
            match mask {
                1 => next_vert[e0 as usize] = e3,
                2 => next_vert[e1 as usize] = e0,
                3 => next_vert[e1 as usize] = e3,
                4 => next_vert[e2 as usize] = e1,
                5 => {
                    let xc = x0 + (i as f64 + 0.5) * hx;
                    let yc = y0 + (j as f64 + 0.5) * hy;
                    if f([xc, yc, z]) < 0.0 {
                        next_vert[e0 as usize] = e1;
                        next_vert[e2 as usize] = e3;
                    } else {
                        next_vert[e0 as usize] = e3;
                        next_vert[e2 as usize] = e1;
                    }
                }
                6 => next_vert[e2 as usize] = e0,
                7 => next_vert[e2 as usize] = e3,
                8 => next_vert[e3 as usize] = e2,
                9 => next_vert[e0 as usize] = e2,
                10 => {
                    let xc = x0 + (i as f64 + 0.5) * hx;
                    let yc = y0 + (j as f64 + 0.5) * hy;
                    if f([xc, yc, z]) < 0.0 {
                        next_vert[e3 as usize] = e0;
                        next_vert[e1 as usize] = e2;
                    } else {
                        next_vert[e1 as usize] = e0;
                        next_vert[e3 as usize] = e2;
                    }
                }
                11 => next_vert[e1 as usize] = e2,
                12 => next_vert[e3 as usize] = e1,
                13 => next_vert[e0 as usize] = e1,
                14 => next_vert[e3 as usize] = e0,
                _ => unreachable!(),
            }
        }
    }

    // Stitch directed edges into closed polygon loops.
    let mut visited = vec![false; verts.len()];
    let mut polygons = Vec::new();
    let mut open_chains = 0usize;
    for start in 0..verts.len() {
        if visited[start] || next_vert[start] == u32::MAX {
            continue;
        }
        let mut ring = Vec::new();
        let mut cur = start;
        let mut closed = false;
        while !visited[cur] {
            visited[cur] = true;
            ring.push(verts[cur]);
            let nxt = next_vert[cur];
            if nxt == u32::MAX {
                break;
            }
            if nxt as usize == start {
                closed = true;
                break;
            }
            cur = nxt as usize;
        }
        if closed {
            if ring.len() >= 3 && polygon_signed_area(&ring).abs() > 1e-12 {
                polygons.push(ring);
            }
        } else {
            open_chains += 1;
        }
    }

    SliceLayer {
        z,
        polygons,
        open_chains,
    }
}

/// Extract a full stack of 2D cross-section layers across `[lo[2], hi[2]]` at layer height
/// `layer_h` (mm) and X/Y sampling spacing `h` (mm), evaluated in parallel with rayon.
pub fn slice_stack(
    f: impl Fn(V3) -> f64 + Sync,
    lo: V3,
    hi: V3,
    layer_h: f64,
    h: f64,
) -> SliceStack {
    let layer_h = layer_h.max(1e-4);
    let h = h.max(1e-4);
    let z_span = (hi[2] - lo[2]).max(layer_h);
    let n_layers = (z_span / layer_h).ceil().max(1.0) as usize;
    let layers: Vec<SliceLayer> = (0..n_layers)
        .into_par_iter()
        .map(|k| {
            let z = (lo[2] + (k as f64 + 0.5) * layer_h).min(hi[2] - 1e-6 * layer_h);
            slice_field(&f, lo, hi, z, h)
        })
        .collect();
    SliceStack {
        z_bottom: lo[2],
        layer_h,
        h,
        origin: lo,
        layers,
    }
}

/// Write a [`SliceStack`] as a 3MF archive using the 3MF Slice Extension
/// (`http://schemas.microsoft.com/3dmanufacturing/slice/2015/07`).
///
/// Slice coordinates inside `<s:slicestack>` are translated by `-stack.origin` into the
/// positive octant as required by the 3MF specification, and the `<build><item>` carries
/// the corresponding translation transform so readers reconstruct exact model coordinates.
pub fn write_3mf_slicestack(name: &str, material: &str, stack: &SliceStack) -> Vec<u8> {
    let types = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
        "<Default Extension=\"rels\" \
         ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
        "<Default Extension=\"model\" \
         ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>",
        "</Types>"
    );
    let rels = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        "<Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" \
         Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/>",
        "</Relationships>"
    );

    let mut xml = String::new();
    let _ = write!(
        xml,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <model unit=\"millimeter\" xml:lang=\"en-US\" \
         xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\" \
         xmlns:s=\"http://schemas.microsoft.com/3dmanufacturing/slice/2015/07\" \
         requiredextensions=\"s\">\n\
         <resources>"
    );
    if !material.is_empty() {
        let _ = write!(
            xml,
            "<basematerials id=\"1\"><base name=\"{}\" displaycolor=\"#DFDEDA\"/></basematerials>",
            esc_xml(material)
        );
    }
    let ox = stack.origin[0];
    let oy = stack.origin[1];
    let oz = stack.z_bottom;
    let _ = write!(
        xml,
        "<s:slicestack id=\"2\" zbottom=\"{:.6}\">",
        (stack.z_bottom - oz).max(0.0)
    );
    for (li, layer) in stack.layers.iter().enumerate() {
        let ztop = ((li + 1) as f64 * stack.layer_h).max(layer.z - oz);
        let _ = write!(xml, "<s:slice ztop=\"{:.6}\"><s:vertices>", ztop);
        for poly in &layer.polygons {
            for p in poly {
                let _ = write!(
                    xml,
                    "<s:vertex x=\"{:.6}\" y=\"{:.6}\"/>",
                    p[0] - ox,
                    p[1] - oy
                );
            }
        }
        xml.push_str("</s:vertices>");
        let mut base_v = 0usize;
        for poly in &layer.polygons {
            let n = poly.len();
            if n >= 3 {
                let _ = write!(xml, "<s:polygon startv=\"{base_v}\">");
                for k in 1..n {
                    let _ = write!(xml, "<s:segment v2=\"{}\"/>", base_v + k);
                }
                let _ = write!(xml, "<s:segment v2=\"{base_v}\"/>");
                xml.push_str("</s:polygon>");
            }
            base_v += n;
        }
        xml.push_str("</s:slice>");
    }
    xml.push_str("</s:slicestack>");

    let name_attr = if name.is_empty() {
        String::new()
    } else {
        format!(" name=\"{}\"", esc_xml(name))
    };
    let mat_attr = if material.is_empty() {
        String::new()
    } else {
        String::from(" pid=\"1\" pindex=\"0\"")
    };
    let _ = write!(
        xml,
        "<object id=\"3\" type=\"model\"{name_attr}{mat_attr} s:slicestackid=\"2\" slicestackid=\"2\">\
         <mesh><vertices/><triangles/></mesh></object>\
         </resources>\n\
         <build><item objectid=\"3\" transform=\"1 0 0 0 1 0 0 0 1 {:.6} {:.6} {:.6}\"/></build>\n\
         </model>\n",
        ox, oy, oz
    );

    let mut out = Vec::new();
    let mut records = Vec::new();
    stream_entry(
        &mut out,
        "[Content_Types].xml",
        types.as_bytes(),
        &mut records,
    );
    stream_entry(&mut out, "_rels/.rels", rels.as_bytes(), &mut records);
    let mut z = ZipStream::start(&mut out, "3D/3dmodel.model");
    z.push(xml.as_bytes());
    records.push(z.finish());
    write_central(&mut out, &records);
    out
}

/// Read a 3MF Slice Extension archive produced by [`write_3mf_slicestack`] (or another 3MF
/// Slice Extension writer) back into a [`SliceStack`].
pub fn read_3mf_slicestack(bytes: &[u8]) -> Result<SliceStack, String> {
    let entries = read_zip(bytes)?;
    let model = entries
        .iter()
        .find(|e| e.name == "3D/3dmodel.model" || e.name == "/3D/3dmodel.model")
        .ok_or_else(|| "3MF archive has no 3D/3dmodel.model entry".to_string())?;
    let text = std::str::from_utf8(&model.data)
        .map_err(|_| "3D/3dmodel.model is not valid UTF-8".to_string())?;
    parse_slicestack_xml(text)
}

fn parse_slicestack_xml(text: &str) -> Result<SliceStack, String> {
    // First pass: check for build item transform translation (ox, oy, oz).
    let mut ox = 0.0f64;
    let mut oy = 0.0f64;
    let mut oz = 0.0f64;
    for tag in scan_tags(text) {
        if (tag.name == "item" || tag.name.ends_with(":item")) && !tag.closing {
            if let Some(t) = tag.attr("transform") {
                let nums: Vec<f64> = t
                    .split_whitespace()
                    .filter_map(|w| w.parse::<f64>().ok())
                    .collect();
                if nums.len() == 12 {
                    ox = nums[9];
                    oy = nums[10];
                    oz = nums[11];
                }
            }
        }
    }

    let mut z_bottom_rel = 0.0f64;
    let mut saw_stack = false;
    let mut raw_slices: Vec<(f64, Vec<Vec<[f64; 2]>>)> = Vec::new();
    let mut cur_ztop = 0.0f64;
    let mut cur_verts: Vec<[f64; 2]> = Vec::new();
    let mut cur_polys: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut cur_poly_start: Option<usize> = None;
    let mut cur_poly_indices: Vec<usize> = Vec::new();

    for tag in scan_tags(text) {
        let local = tag
            .name
            .rsplit_once(':')
            .map(|(_, l)| l)
            .unwrap_or(&tag.name);
        match (local, tag.closing) {
            ("slicestack", false) => {
                saw_stack = true;
                if let Some(zb) = tag.attr("zbottom") {
                    z_bottom_rel = zb
                        .parse::<f64>()
                        .map_err(|_| format!("invalid slicestack zbottom '{zb}'"))?;
                }
            }
            ("slice", false) => {
                let zt = tag
                    .attr("ztop")
                    .ok_or_else(|| "<s:slice> missing ztop attribute".to_string())?;
                cur_ztop = zt
                    .parse::<f64>()
                    .map_err(|_| format!("invalid slice ztop '{zt}'"))?;
                cur_verts.clear();
                cur_polys.clear();
            }
            ("slice", true) => {
                raw_slices.push((cur_ztop, std::mem::take(&mut cur_polys)));
                cur_verts.clear();
            }
            ("vertex", false) if saw_stack => {
                if let (Some(xs), Some(ys)) = (tag.attr("x"), tag.attr("y")) {
                    if tag.attr("z").is_none() {
                        let x = xs
                            .parse::<f64>()
                            .map_err(|_| format!("invalid slice vertex x '{xs}'"))?;
                        let y = ys
                            .parse::<f64>()
                            .map_err(|_| format!("invalid slice vertex y '{ys}'"))?;
                        cur_verts.push([x + ox, y + oy]);
                    }
                }
            }
            ("polygon", false) => {
                let sv = tag
                    .attr("startv")
                    .ok_or_else(|| "<s:polygon> missing startv attribute".to_string())?
                    .parse::<usize>()
                    .map_err(|_| "invalid polygon startv".to_string())?;
                cur_poly_start = Some(sv);
                cur_poly_indices.clear();
                cur_poly_indices.push(sv);
            }
            ("segment", false) => {
                let v2 = tag
                    .attr("v2")
                    .ok_or_else(|| "<s:segment> missing v2 attribute".to_string())?
                    .parse::<usize>()
                    .map_err(|_| "invalid segment v2".to_string())?;
                cur_poly_indices.push(v2);
            }
            ("polygon", true) => {
                if let Some(startv) = cur_poly_start.take() {
                    if cur_poly_indices.len() >= 2 && *cur_poly_indices.last().unwrap() == startv {
                        cur_poly_indices.pop();
                    }
                    let mut ring = Vec::with_capacity(cur_poly_indices.len());
                    for &idx in &cur_poly_indices {
                        let pt = *cur_verts.get(idx).ok_or_else(|| {
                            format!(
                                "slice polygon vertex index {idx} out of bounds ({})",
                                cur_verts.len()
                            )
                        })?;
                        ring.push(pt);
                    }
                    cur_polys.push(ring);
                }
                cur_poly_indices.clear();
            }
            _ => {}
        }
    }

    if !saw_stack {
        return Err("3MF model contains no <s:slicestack>".into());
    }
    let z_bottom = oz + z_bottom_rel;
    let layer_h = if raw_slices.len() >= 2 {
        (raw_slices[1].0 - raw_slices[0].0).abs().max(1e-4)
    } else if let Some((zt, _)) = raw_slices.first() {
        (*zt - z_bottom_rel).abs().max(1e-4)
    } else {
        1.0
    };
    let layers = raw_slices
        .into_iter()
        .enumerate()
        .map(|(k, (_ztop, polygons))| SliceLayer {
            z: z_bottom + (k as f64 + 0.5) * layer_h,
            polygons,
            open_chains: 0,
        })
        .collect();

    Ok(SliceStack {
        z_bottom,
        layer_h,
        h: 0.0,
        origin: [ox, oy, z_bottom],
        layers,
    })
}

fn esc_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

struct XmlTag {
    name: String,
    attrs: HashMap<String, String>,
    closing: bool,
}

impl XmlTag {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.get(k).map(|s| s.as_str())
    }
}

fn scan_tags(text: &str) -> Vec<XmlTag> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut pos = 0usize;
    while pos < bytes.len() {
        if bytes[pos] != b'<' {
            pos += 1;
            continue;
        }
        if text[pos..].starts_with("<!--") {
            if let Some(e) = text[pos..].find("-->") {
                pos += e + 3;
                continue;
            }
            break;
        }
        if text[pos..].starts_with("<?") {
            if let Some(e) = text[pos..].find("?>") {
                pos += e + 2;
                continue;
            }
            break;
        }
        let start = pos + 1;
        let mut i = start;
        let mut quote = false;
        while i < bytes.len() {
            if bytes[i] == b'"' {
                quote = !quote;
            } else if !quote && bytes[i] == b'>' {
                break;
            }
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let inner = text[start..i].trim();
        pos = i + 1;
        let (closing, inner) = match inner.strip_prefix('/') {
            Some(r) => (true, r.trim_start()),
            None => (false, inner),
        };
        let (self_closing, inner) = match inner.strip_suffix('/') {
            Some(r) => (true, r.trim_end()),
            None => (false, inner),
        };
        let name_end = inner
            .find(|c: char| c.is_whitespace() || c == '=')
            .unwrap_or(inner.len());
        let name = inner[..name_end].to_string();
        let rest = &inner[name_end..];
        let mut attrs = HashMap::new();
        let b = rest.as_bytes();
        let mut k = 0usize;
        while k < b.len() {
            while k < b.len() && (b[k] as char).is_whitespace() {
                k += 1;
            }
            if k >= b.len() {
                break;
            }
            let ks = k;
            while k < b.len() && b[k] != b'=' && !(b[k] as char).is_whitespace() {
                k += 1;
            }
            let key = rest[ks..k].to_string();
            while k < b.len() && (b[k] as char).is_whitespace() {
                k += 1;
            }
            if k < b.len() && b[k] == b'=' {
                k += 1;
                while k < b.len() && (b[k] as char).is_whitespace() {
                    k += 1;
                }
                if k < b.len() && b[k] == b'"' {
                    k += 1;
                    let vs = k;
                    while k < b.len() && b[k] != b'"' {
                        k += 1;
                    }
                    let val = rest[vs..k].to_string();
                    if k < b.len() {
                        k += 1;
                    }
                    attrs.insert(key, val);
                }
            }
        }
        out.push(XmlTag {
            name: name.clone(),
            attrs,
            closing,
        });
        if self_closing && !closing {
            out.push(XmlTag {
                name,
                attrs: HashMap::new(),
                closing: true,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sphere_slice_areas_and_3mf_roundtrip() {
        let r = 10.0f64;
        let f = |p: V3| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - r;
        let lo = [-r, -r, -r];
        let hi = [r, r, r];
        for &z in &[-6.0, -3.0, 0.0, 4.5, 8.0] {
            let layer = slice_field(f, lo, hi, z, 0.25);
            assert_eq!(layer.open_chains, 0);
            assert_eq!(layer.polygons.len(), 1);
            let exact = std::f64::consts::PI * (r * r - z * z);
            let rel_err = (layer.area() - exact).abs() / exact;
            assert!(
                rel_err < 0.005,
                "z={z}: area={} vs exact={exact} (rel_err={rel_err})",
                layer.area()
            );
        }
        let stack = slice_stack(f, lo, hi, 2.0, 0.4);
        assert_eq!(stack.open_chains(), 0);
        let bytes = write_3mf_slicestack("sphere", "PLA", &stack);
        let back = read_3mf_slicestack(&bytes).expect("read_3mf_slicestack");
        assert_eq!(back.layers.len(), stack.layers.len());
        for (a, b) in stack.layers.iter().zip(back.layers.iter()) {
            assert_eq!(a.polygons.len(), b.polygons.len());
            assert!((a.area() - b.area()).abs() < 1e-3);
        }
    }
}
