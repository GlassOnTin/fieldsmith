//! The `.fsmz` sidecar (goal M6, step 7): everything a document's imported mesh grids
//! carry, next to the readable `.fsm`. Entries: `doc.fsm` (the printed document),
//! `manifest.json` (one record per distinct mesh hash with the grid's placement and the
//! payload CRCs), and `<hash>.dist` / `<hash>.patch`, where the distance samples and the
//! optional patch grid live as little-endian row-major arrays (x fastest, the `Grid3`
//! layout).
//!
//! [`save`] re-hydrates nothing: every hash the document names must already be live in the
//! mesh-grid registry — true right after an import or a [`load`] — and a missing one is an
//! error naming the hash and the remedies. [`load`] parses the document, cross-checks it
//! against the manifest (units, counters, patch presence, unreferenced entries) and
//! re-hydrates the registry, bit-identical to what was saved.

use fieldsmith_doc::{Document, Node, NodeKind};
use fieldsmith_field::expr::Rigid;
use fieldsmith_field::lattice::Grid3;
use fieldsmith_field::meshfield::{
    extend_key, extend_key_face, hydrate, key_from_hex, lookup, swept_face_field,
    swept_patch_field, MeshField, MeshMeta, NO_PATCH,
};
use fieldsmith_field::Aabb;
use fieldsmith_mesh::zip::{crc32, read_zip, ZipWriter};
use serde::{Deserialize, Serialize};

const SIDECAR_VERSION: u32 = 1;
const DOC_ENTRY: &str = "doc.fsm";
const MANIFEST_ENTRY: &str = "manifest.json";

/// One manifest record, sourced from the mesh node and its hydrated grid at save time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManifestMesh {
    /// 16 hex digits, matching the node's `hash` and the data entries.
    pub hash: String,
    pub units: String,
    pub up: String,
    pub voxel: f64,
    /// Grid origin in mm (node-centred corner cell).
    pub origin: [f64; 3],
    /// Baked grid spacing in mm.
    pub spacing: f64,
    pub dims: [usize; 3],
    pub tris: u32,
    pub patches: u32,
    pub open_edges: u32,
    pub dist_crc32: u32,
    pub patch: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch_crc32: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub v: u32,
    pub meshes: Vec<ManifestMesh>,
    /// Region-driven extend records (goal M6, step 8). Writers that predate extends leave
    /// it out; the serde default keeps those readable.
    #[serde(default)]
    pub extends: Vec<ManifestExtend>,
}

/// One manifest record for a region-driven extend: the derived sweep grid keyed by its
/// recipe (parent mesh, patch ids, direction, length). The document's node repeats the
/// region / dir / len and is cross-checked against this record on load; the record adds
/// the grid placement the node does not store.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManifestExtend {
    /// The extend node's id.
    pub id: String,
    /// 16 hex digits — the derived grid's key; the data entry is `ext-<hash>.dist`.
    pub hash: String,
    pub region: String,
    /// Hash of the parent mesh node the sweep is derived from.
    pub parent_hash: String,
    /// The region's patch ids naming the parent, sorted and deduped. A face-rule sweep
    /// (the geometric selector) keeps this empty and records the axis in `face_axis`.
    pub patch_ids: Vec<u32>,
    /// The face-rule sweep's unit axis, in the parent's LOCAL grid frame — the frame the
    /// derivation hashed. Absent for patch sweeps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_axis: Option<[f64; 3]>,
    pub dir: [f64; 3],
    pub len: f64,
    /// Derived grid placement, bounds and payload (as for meshes: a node-centred f32
    /// grid, x fastest).
    pub origin: [f64; 3],
    pub spacing: f64,
    pub dims: [usize; 3],
    pub bbox: [f64; 6],
    pub dist_crc32: u32,
}

/// A loaded sidecar: the document plus its parsed manifest (the registry is hydrated as a
/// side effect).
#[derive(Clone, Debug, Serialize)]
pub struct Loaded {
    pub doc: Document,
    pub manifest: Manifest,
}

// ---------------------------------------------------------------------------------------------
// Array payloads

/// Distance bytes: little-endian f32, row-major x fastest.
pub fn encode_f32(data: &[f32]) -> Vec<u8> {
    data.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Patch bytes: little-endian u16, same layout.
pub fn encode_u16(data: &[u16]) -> Vec<u8> {
    data.iter().flat_map(|p| p.to_le_bytes()).collect()
}

/// A trailing partial sample is refused rather than silently dropped.
pub fn decode_f32(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if !bytes.len().is_multiple_of(4) {
        return Err(format!(
            "{} bytes is not a whole number of f32 samples",
            bytes.len()
        ));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect())
}

pub fn decode_u16(bytes: &[u8]) -> Result<Vec<u16>, String> {
    if !bytes.len().is_multiple_of(2) {
        return Err(format!(
            "{} bytes is not a whole number of u16 samples",
            bytes.len()
        ));
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes(c.try_into().unwrap()))
        .collect())
}

// ---------------------------------------------------------------------------------------------
// Walking

/// Every mesh leaf of the document, in document order, groups recursed.
fn mesh_leaves(nodes: &[Node], out: &mut Vec<Node>) {
    for n in nodes {
        match &n.kind {
            NodeKind::Group { children, .. } => mesh_leaves(children, out),
            NodeKind::Mesh { .. } => out.push(n.clone()),
            _ => {}
        }
    }
}

fn node_hash(n: &Node) -> Option<&str> {
    match &n.kind {
        NodeKind::Mesh { hash, .. } => Some(hash),
        _ => None,
    }
}

/// Every extend leaf of the document, in document order, groups recursed.
fn extend_leaves(nodes: &[Node], out: &mut Vec<Node>) {
    for n in nodes {
        match &n.kind {
            NodeKind::Group { children, .. } => extend_leaves(children, out),
            NodeKind::Extend { .. } => out.push(n.clone()),
            _ => {}
        }
    }
}

/// What a swept parent's selectors name in the [`Document`]: a set of patch ids, or a face
/// direction held in the parent's LOCAL grid frame (the frame the derivation hashes).
/// Mirrors `fieldsmith-field/src/expr.rs SweptFrom` — kept textually parallel so the two
/// recipes cannot drift apart unnoticed.
enum SweptFrom {
    Patches(Vec<u16>),
    Face([f64; 3]),
}

/// One extend node's parent arms in selector order — per (parent hash, parent node id,
/// swept source) triple, patch selectors on one parent unioning their ids and face
/// selectors grouping by local axis, the grouping `fieldsmith-field/src/expr.rs
/// compile_extend` does. The face axis is held in the parent's local frame via the mesh
/// node's own transform, which is also what maps the doc-frame sweep direction (each
/// caller looks the parent node up by id for it). None means the node names an unknown
/// region.
fn extend_parents(doc: &Document, region: &str) -> Option<Vec<(String, String, SweptFrom)>> {
    let r = doc.regions.iter().find(|r| r.id == region)?;
    let mut parents: Vec<(String, String, SweptFrom)> = Vec::new();
    for s in &r.selectors {
        let Some(node) = doc.find(&s.shape) else {
            continue;
        };
        let NodeKind::Mesh { hash, .. } = &node.kind else {
            continue;
        };
        if let Some(p) = s.patch {
            if p == 0 || p >= u16::MAX as u32 {
                continue;
            }
            match parents.iter_mut().find(|(h, id, from)| {
                *h == *hash && *id == node.id && matches!(from, SweptFrom::Patches(_))
            }) {
                Some((_, _, SweptFrom::Patches(ids))) => {
                    if !ids.contains(&(p as u16)) {
                        ids.push(p as u16);
                    }
                }
                _ => parents.push((
                    hash.clone(),
                    node.id.clone(),
                    SweptFrom::Patches(vec![p as u16]),
                )),
            }
        } else if let Some(ax) = s.face.axis() {
            // Mapped into the parent's local frame — the same mapping the compile side
            // hashes, so the recipes and their keys agree frame for frame.
            let ax_leaf = Rigid::from_doc(&node.transform).to_local_dir(ax);
            let duplicate = parents.iter().any(|(h, id, from)| {
                *h == *hash
                    && *id == node.id
                    && matches!(from, SweptFrom::Face(a2) if *a2 == ax_leaf)
            });
            if !duplicate {
                parents.push((hash.clone(), node.id.clone(), SweptFrom::Face(ax_leaf)));
            }
        }
    }
    Some(parents)
}

// ---------------------------------------------------------------------------------------------
// Save

/// Write the sidecar for `doc`: the printed document, the manifest and one dist/patch
/// entry per distinct (hydrated) hash. The document's grid contents are exact f32/u16
/// round-trips, so save → load → save is byte-identical.
pub fn save(doc: &Document) -> Result<Vec<u8>, String> {
    let mut leaves = Vec::new();
    mesh_leaves(doc.root.as_slice(), &mut leaves);

    // Distinct hashes in document order, one registry lookup apiece.
    let mut hashes: Vec<String> = Vec::new();
    for hash in leaves.iter().filter_map(node_hash) {
        if !hashes.contains(&hash.to_string()) {
            hashes.push(hash.to_string());
        }
    }
    let mut fields: Vec<(String, MeshField)> = Vec::with_capacity(hashes.len());
    for hash in &hashes {
        let key = key_from_hex(hash);
        if key == 0 {
            return Err(format!(
                "mesh hash '{hash}' is not a 16-hex fingerprint — re-import the part"
            ));
        }
        let f = lookup(key).ok_or_else(|| {
            format!("mesh '{hash}' is not hydrated — re-import the source or load its .fsmz sidecar, then save again")
        })?;
        fields.push((hash.clone(), (*f).clone()));
    }

    // Manifest record per hash, sourced from the first node naming it; nodes sharing a
    // hash were produced by the same import recipe (unit/up/voxel are in the key salt),
    // so the copies agree.
    let mut manifest = Manifest {
        v: SIDECAR_VERSION,
        meshes: Vec::new(),
        extends: Vec::new(),
    };
    for (hash, f) in &fields {
        let node = &leaves[leaves
            .iter()
            .position(|n| node_hash(n) == Some(hash.as_str()))
            .unwrap()];
        let NodeKind::Mesh {
            units,
            up,
            voxel,
            bbox,
            tris,
            patches,
            open_edges,
            ..
        } = node.kind.clone()
        else {
            unreachable!("leaf filtered to mesh nodes")
        };
        if bbox != f_dist_box(f) {
            return Err(format!(
                "mesh '{hash}' node bounds disagree with the hydrated grid — re-import the part"
            ));
        }
        if (tris, patches, open_edges) != (f.meta.tris, f.meta.patches, f.meta.open_edges) {
            return Err(format!(
                "mesh '{hash}' node report ({tris} tris, {patches} patches, {open_edges} open edges) disagrees with the hydrated grid"
            ));
        }
        manifest.meshes.push(ManifestMesh {
            hash: hash.clone(),
            units,
            up,
            voxel,
            origin: f.dist.origin,
            spacing: f.dist.spacing,
            dims: f.dist.dims,
            tris,
            patches,
            open_edges,
            dist_crc32: crc32(&encode_f32(&f.dist.data)),
            patch: f.patch.is_some(),
            patch_crc32: f.patch.as_ref().map(|p| crc32(&encode_u16(p))),
        });
    }

    // Region-driven extends: resolve each node's region into its parent meshes' patch sets,
    // derive the swept grid off the hydrated parent (the same recipe the compile side runs)
    // and carry one dist entry per distinct sweep; two extend nodes with the same recipe
    // share the entry and record their CRCs against it.
    let mut extend_payloads: Vec<(u64, Vec<u8>)> = Vec::new();
    for n in {
        let mut v = Vec::new();
        extend_leaves(doc.root.as_slice(), &mut v);
        v
    } {
        let NodeKind::Extend { region, dir, len } = &n.kind else {
            unreachable!("leaves filtered to extend nodes")
        };
        let parents = extend_parents(doc, region)
            .ok_or_else(|| format!("extend '{}' names unknown region '{region}'", n.id))?;
        if parents.is_empty() {
            return Err(format!(
                "extend '{}' region '{region}' names no mesh patch or face to sweep — name a mesh patch or face, or drop the extend",
                n.id
            ));
        }
        for (parent_hash, parent_id, from) in parents {
            let pkey = key_from_hex(&parent_hash);
            if pkey == 0 || lookup(pkey).is_none() {
                return Err(format!(
                    "mesh '{parent_hash}' backing extend '{}' is not hydrated — re-import the source or load its .fsmz sidecar, then save again",
                    n.id
                ));
            }
            let f = lookup(pkey).expect("checked above");
            // The hydrated grid lives in the parent's local frame; doc-frame dir and axes
            // map through the PARENT node's transform first — exactly what the compile
            // side does before sweeping and hashing.
            let parent_node = doc
                .find(&parent_id)
                .expect("the parent node came from the same document");
            let rig = Rigid::from_doc(&parent_node.transform);
            let dir_leaf = rig.to_local_dir(*dir);
            let (derived, key, patch_ids32, face_axis) = match &from {
                SweptFrom::Patches(ids) => {
                    let d = swept_patch_field(&f, dir_leaf, *len, ids)
                        .map_err(|e| format!("extend '{}' could not be swept: {e}", n.id))?;
                    let key = extend_key(&parent_hash, ids, dir_leaf, *len, d.dist.spacing);
                    let ids32 = ids.iter().map(|&p| p as u32).collect();
                    (d, key, ids32, None)
                }
                SweptFrom::Face(axis) => {
                    let d = swept_face_field(&f, dir_leaf, *len, *axis)
                        .map_err(|e| format!("extend '{}' could not be swept: {e}", n.id))?;
                    let key = extend_key_face(&parent_hash, *axis, dir_leaf, *len, d.dist.spacing);
                    (d, key, Vec::new(), Some(*axis))
                }
            };
            let dist_bytes = encode_f32(&derived.dist.data);
            if !extend_payloads.iter().any(|(k, _)| *k == key) {
                extend_payloads.push((key, dist_bytes));
            }
            manifest.extends.push(ManifestExtend {
                id: n.id.clone(),
                hash: format!("{key:016x}"),
                region: region.clone(),
                parent_hash: parent_hash.clone(),
                patch_ids: patch_ids32,
                face_axis,
                dir: *dir,
                len: *len,
                origin: derived.dist.origin,
                spacing: derived.dist.spacing,
                dims: derived.dist.dims,
                bbox: f_dist_box(&derived),
                dist_crc32: crc32(&encode_f32(&derived.dist.data)),
            });
        }
    }

    let manifest_bytes = serde_json::to_string_pretty(&manifest)
        .map_err(|_| "the manifest does not serialize (non-finite numbers?)".to_string())?;
    let mut z = ZipWriter::new();
    z.entry(DOC_ENTRY, print_fsm(doc).as_bytes());
    z.entry(MANIFEST_ENTRY, manifest_bytes.as_bytes());
    for (hash, f) in &fields {
        z.entry(&format!("{hash}.dist"), &encode_f32(&f.dist.data));
        if let Some(p) = &f.patch {
            z.entry(&format!("{hash}.patch"), &encode_u16(p));
        }
    }
    for (key, bytes) in &extend_payloads {
        z.entry(&format!("ext-{key:016x}.dist"), bytes);
    }
    Ok(z.finish())
}

fn f_dist_box(f: &MeshField) -> [f64; 6] {
    let b = f.meta.bbox;
    [b.lo[0], b.lo[1], b.lo[2], b.hi[0], b.hi[1], b.hi[2]]
}

fn print_fsm(doc: &Document) -> String {
    fieldsmith_doc::fsm::print(doc)
}

// ---------------------------------------------------------------------------------------------
// Load

/// Read a sidecar: inflate the entries, parse and cross-check the document against the
/// manifest (version, counters, patch presence, unreferenced payloads) and re-hydrate the
/// registry per manifest hash.
pub fn load(bytes: &[u8]) -> Result<Loaded, String> {
    let entries = read_zip(bytes).map_err(|e| format!("the sidecar does not read: {e}"))?;
    let (mut docs, mut manifests, mut data) = (Vec::new(), Vec::new(), Vec::new());
    for e in entries {
        match e.name.as_str() {
            DOC_ENTRY => docs.push(e.data),
            MANIFEST_ENTRY => manifests.push(e.data),
            _ => data.push((e.name, e.data)),
        }
    }
    let doc_bytes = one(&docs, DOC_ENTRY)?;
    let manifest_bytes = one(&manifests, MANIFEST_ENTRY)?;
    let doc_text =
        std::str::from_utf8(&doc_bytes).map_err(|_| format!("{DOC_ENTRY} is not UTF-8 text"))?;
    let doc = fieldsmith_doc::fsm::parse(doc_text)
        .map_err(|e| format!("{DOC_ENTRY} does not parse: {e}"))?;
    let manifest_text = std::str::from_utf8(&manifest_bytes)
        .map_err(|_| format!("{MANIFEST_ENTRY} is not UTF-8 text"))?;
    let manifest: Manifest = serde_json::from_str(manifest_text)
        .map_err(|e| format!("{MANIFEST_ENTRY} does not parse: {e}"))?;
    if manifest.v != SIDECAR_VERSION {
        return Err(format!(
            "sidecar manifest version {} is not supported (this tool writes {})",
            manifest.v, SIDECAR_VERSION
        ));
    }

    let mut leaves = Vec::new();
    mesh_leaves(doc.root.as_slice(), &mut leaves);
    let mut seen_hashes = std::collections::HashSet::new();
    for m in &manifest.meshes {
        if !seen_hashes.insert(m.hash.clone()) {
            return Err(format!("the manifest lists mesh '{}' twice", m.hash));
        }
        let key = key_from_hex(&m.hash);
        if key == 0 {
            return Err(format!(
                "manifest mesh hash '{}' is not a 16-hex fingerprint",
                m.hash
            ));
        }

        // Payload: inflate, CRC against the manifest, exact cell count.
        let dname = format!("{}.dist", m.hash);
        let dist_bytes = take(&mut data, &dname)?;
        if crc32(&dist_bytes) != m.dist_crc32 {
            return Err(format!("entry '{dname}' fails its manifest crc32"));
        }
        let dist = decode_f32(&dist_bytes)
            .map_err(|e| format!("entry '{dname}': {e}"))
            .and_then(|d| check_len(d, &m.dims, "dist", &dname))?;
        let patch = if let Some(pc) = m.patch_crc32 {
            let pname = format!("{}.patch", m.hash);
            let pbytes = take(&mut data, &pname)?;
            if crc32(&pbytes) != pc {
                return Err(format!("entry '{pname}' fails its manifest crc32"));
            }
            Some(
                decode_u16(&pbytes)
                    .map_err(|e| format!("entry '{pname}': {e}"))
                    .and_then(|p| check_len(p, &m.dims, "patch", &pname))?,
            )
        } else {
            None
        };

        // Cross-check against the document's nodes naming this hash.
        let nodes: Vec<&Node> = leaves
            .iter()
            .filter(|n| node_hash(n) == Some(m.hash.as_str()))
            .collect();
        if nodes.is_empty() {
            return Err(format!(
                "the manifest lists mesh '{}' but no node in {DOC_ENTRY} references it",
                m.hash
            ));
        }
        for n in &nodes {
            check_node(n, m)?;
        }

        let n0 = nodes[0].kind.clone();
        let NodeKind::Mesh { bbox, .. } = n0 else {
            unreachable!("filtered to mesh nodes")
        };
        let meta = MeshMeta {
            bbox: Aabb {
                lo: bbox[0..3].try_into().unwrap(),
                hi: bbox[3..6].try_into().unwrap(),
            },
            tris: m.tris,
            patches: m.patches,
            open_edges: m.open_edges,
        };
        // Patch grid sanity: out-of-range ids are corrupt data (NO_PATCH means unclaimed).
        if patch.as_ref().is_some_and(|p| {
            p.iter()
                .any(|&id| id != NO_PATCH && (id == 0 || id as u32 > m.patches))
        }) {
            return Err(format!(
                "entry '{}.patch' carries a patch id outside 1..={}",
                m.hash, m.patches
            ));
        }
        let f = MeshField {
            dist: std::sync::Arc::new(Grid3 {
                origin: m.origin,
                spacing: m.spacing,
                dims: m.dims,
                data: dist,
            }),
            patch: patch.map(std::sync::Arc::new),
            meta,
        };
        hydrate(key, f);
    }

    // And the document side of that symmetry: every mesh leaf names a manifest record. One
    // the manifest dropped would otherwise load with its grid silently unhydrated (the
    // extend side checks the same direction below).
    for n in &leaves {
        if let Some(hash) = node_hash(n) {
            if !manifest.meshes.iter().any(|m| m.hash == hash) {
                return Err(format!(
                    "mesh node '{}' (hash '{hash}') has no record in {MANIFEST_ENTRY} — the sidecar predates or dropped this import",
                    n.id
                ));
            }
        }
    }

    // Extend records: each record is one (extend node, parent arm) pair — a multi-arm
    // extend legitimately has several records sharing its node id — cross-checked against
    // the recipe (region, parent, patch ids or face axis, dir, len) recomputed from the
    // document, then hydrated. As with mesh payloads the CRC and cross-checks are trusted
    // rather than re-derived; a change to the sweep derivation bumps SIDECAR_VERSION.
    let mut ext_nodes = Vec::new();
    extend_leaves(doc.root.as_slice(), &mut ext_nodes);
    let mut seen_ext_records: std::collections::HashSet<(String, String, String)> =
        std::collections::HashSet::new();
    for x in &manifest.extends {
        let trip = (x.id.clone(), x.parent_hash.clone(), x.hash.clone());
        if !seen_ext_records.insert(trip) {
            return Err(format!("the manifest lists extend '{}' twice", x.id));
        }
    }
    let mut ext_payloads: Vec<(String, Vec<u8>)> = Vec::new();
    for n in &ext_nodes {
        let NodeKind::Extend { region, dir, len } = &n.kind else {
            unreachable!("filtered to extend nodes")
        };
        let records: Vec<&ManifestExtend> =
            manifest.extends.iter().filter(|x| x.id == n.id).collect();
        if records.is_empty() {
            return Err(format!(
                "extend node '{id}' has no record in {MANIFEST_ENTRY} — the sidecar predates or dropped this sweep",
                id = n.id
            ));
        }
        if records
            .iter()
            .any(|x| x.region != *region || x.dir != *dir || x.len != *len)
        {
            return Err(format!(
                "extend node '{}' disagrees with the manifest record's region, dir or len",
                n.id
            ));
        }
        // The parent, patch ids and face axes are recomputed from the document the way the
        // compiler does — `extend_parents` mirrors fieldsmith-field's compile-side
        // collection; each record must follow from one arm and every arm must have a record.
        let parents = extend_parents(&doc, region)
            .ok_or_else(|| format!("extend node '{}' names unknown region '{region}'", n.id))?;
        if parents.is_empty() {
            return Err(format!(
                "extend node '{}' region '{region}' names no mesh patch or face in {DOC_ENTRY}",
                n.id
            ));
        }
        let mut used = vec![false; records.len()];
        for (parent_hash, parent_id, from) in &parents {
            // Each document arm pairs with exactly one record — patch arms by their sorted
            // id set, face arms by the parent-local axis — the same recipe the save wrote.
            let (arm_ids32, arm_axis) = match from {
                SweptFrom::Patches(ids) => {
                    (ids.iter().map(|&p| p as u32).collect::<Vec<u32>>(), None)
                }
                SweptFrom::Face(axis) => (Vec::new(), Some(*axis)),
            };
            let Some(idx) = records.iter().position(|x| {
                x.parent_hash == *parent_hash && x.patch_ids == arm_ids32 && x.face_axis == arm_axis
            }) else {
                return Err(format!(
                    "extend node '{}' has no manifest record for the arm naming '{}' — the sidecar predates or dropped this sweep",
                    n.id, parent_hash
                ));
            };
            used[idx] = true;
            let x = records[idx];
            let pm = manifest
                .meshes
                .iter()
                .find(|m| m.hash == *parent_hash)
                .ok_or_else(|| {
                    format!(
                        "the manifest lists extend '{}' derived from mesh '{}' which it does not carry",
                        n.id, x.parent_hash
                    )
                })?;
            if x.patch_ids
                .iter()
                .any(|&p| p == 0 || p >= u16::MAX as u32 || p > pm.patches)
            {
                return Err(format!(
                    "the manifest lists extend '{}' with a patch id outside 1..={}",
                    n.id, pm.patches
                ));
            }
            if x.face_axis.is_some_and(|a| {
                a.iter().any(|v| !v.is_finite())
                    || ((a[0] * a[0] + a[1] * a[1] + a[2] * a[2] - 1.0).abs() > 1e-3)
            }) {
                return Err(format!(
                    "the manifest records a non-unit face axis for extend '{}'",
                    n.id
                ));
            }
            if !(x.spacing.is_finite()
                && (x.spacing - pm.spacing).abs() <= 1e-9 * pm.spacing.max(1.0))
            {
                return Err(format!(
                    "the manifest lists extend '{}' at spacing {} outside mesh '{}' at {}",
                    n.id, x.spacing, x.parent_hash, pm.spacing
                ));
            }
            if x.dims.contains(&0)
                || x.origin.iter().any(|v| !v.is_finite())
                || x.bbox.iter().any(|v| !v.is_finite())
            {
                return Err(format!(
                    "the manifest records unusable placement for extend '{}'",
                    n.id
                ));
            }
            let pkey = key_from_hex(&x.parent_hash);
            if pkey == 0 || lookup(pkey).is_none() {
                return Err(format!(
                    "mesh '{}' was not hydrated — the sidecar cannot hydrate the extend '{}'",
                    x.parent_hash, n.id
                ));
            }
            // The derivation hashed the parent's local frame: map the doc-frame dir
            // through the parent node's transform the way the compile side does, then
            // re-derive the key.
            let parent_node = doc
                .find(parent_id)
                .expect("the parent node came from the same document");
            let dir_leaf = Rigid::from_doc(&parent_node.transform).to_local_dir(*dir);
            let key = match arm_axis {
                Some(axis) => extend_key_face(&x.parent_hash, axis, dir_leaf, *len, x.spacing),
                None => {
                    let ids16: Vec<u16> = arm_ids32.iter().map(|&p| p as u16).collect();
                    extend_key(&x.parent_hash, &ids16, dir_leaf, *len, x.spacing)
                }
            };
            if key == 0 || key_from_hex(&x.hash) != key {
                return Err(format!(
                    "the manifest's extend '{}' hash '{}' does not follow from its recipe",
                    n.id, x.hash
                ));
            }
            // Payload: inflate once per entry (shared sweeps), CRC against each record's own
            // manifest numbers, exact cell count. The entry is consumed on first take; later
            // records with the same hash CRC against the cached bytes.
            let dname = format!("ext-{}.dist", x.hash);
            let dist_bytes = match ext_payloads.iter().find(|(e, _)| e == &dname) {
                Some((_, cached)) => cached.clone(),
                None => {
                    let b = take_ext(&mut data, &dname)?;
                    if crc32(&b) != x.dist_crc32 {
                        return Err(format!("entry '{dname}' fails its manifest crc32"));
                    }
                    ext_payloads.push((dname.clone(), b.clone()));
                    b
                }
            };
            let dist = decode_f32(&dist_bytes)
                .map_err(|e| format!("entry '{dname}': {e}"))
                .and_then(|d| check_len(d, &x.dims, "dist", &dname))?;
            let f = MeshField {
                dist: std::sync::Arc::new(Grid3 {
                    origin: x.origin,
                    spacing: x.spacing,
                    dims: x.dims,
                    data: dist,
                }),
                patch: None,
                meta: MeshMeta {
                    bbox: Aabb {
                        lo: x.bbox[0..3].try_into().unwrap(),
                        hi: x.bbox[3..6].try_into().unwrap(),
                    },
                    tris: 0,
                    patches: 0,
                    open_edges: 0,
                },
            };
            hydrate(key, f);
        }
        // And every record must have paired with a document arm — one whose region lost
        // its selector is stale.
        if let Some(idx) = used.iter().position(|&u| !u) {
            return Err(format!(
                "the manifest lists extend '{}' recipe '{}' which the document's region does not name",
                n.id, records[idx].hash
            ));
        }
    }
    for x in &manifest.extends {
        if !ext_nodes.iter().any(|n| n.id == x.id) {
            return Err(format!(
                "the manifest lists extend '{}' but no node in {DOC_ENTRY} references it",
                x.id
            ));
        }
    }

    if let Some((name, _)) = data.first() {
        return Err(format!(
            "the sidecar carries entry '{name}' which neither the document nor the manifest references"
        ));
    }
    Ok(Loaded { doc, manifest })
}

/// Exactly one entry of the given expected name: a duplicate means a corrupt sidecar.
fn one(v: &[Vec<u8>], name: &str) -> Result<Vec<u8>, String> {
    if v.len() > 1 {
        return Err(format!("the sidecar carries {name} twice"));
    }
    v.first()
        .cloned()
        .ok_or_else(|| format!("the sidecar carries no {name} entry"))
}

/// Remove `name` from the walked surplus entries, naming both the expected and the missing.
fn take(data: &mut Vec<(String, Vec<u8>)>, name: &str) -> Result<Vec<u8>, String> {
    match data.iter().position(|(n, _)| n == name) {
        Some(i) => Ok(data.remove(i).1),
        None => Err(format!(
            "the manifest lists mesh '{}' but the sidecar carries no data entry for it",
            name.trim_end_matches(".dist").trim_end_matches(".patch")
        )),
    }
}

/// Same as [`take`], but the entry is an extend sweep's dist and the error says so.
fn take_ext(data: &mut Vec<(String, Vec<u8>)>, name: &str) -> Result<Vec<u8>, String> {
    match data.iter().position(|(n, _)| n == name) {
        Some(i) => Ok(data.remove(i).1),
        None => Err(format!(
            "the manifest lists extend '{}' but the sidecar carries no data entry for it",
            name.trim_start_matches("ext-").trim_end_matches(".dist")
        )),
    }
}

fn check_len<T>(v: Vec<T>, dims: &[usize; 3], what: &str, entry: &str) -> Result<Vec<T>, String> {
    let want = dims[0] * dims[1] * dims[2];
    if v.len() != want {
        return Err(format!(
            "entry '{entry}' ({what}) holds {} samples, the {}×{}×{} grid wants {want}",
            v.len(),
            dims[0],
            dims[1],
            dims[2]
        ));
    }
    Ok(v)
}

/// Node ↔ manifest agreement on the fields the node carries (the manifest is authoritative
/// for grid placement, which the node does not store).
fn check_node(n: &Node, m: &ManifestMesh) -> Result<(), String> {
    let NodeKind::Mesh {
        units,
        up,
        voxel,
        tris,
        patches,
        open_edges,
        ..
    } = &n.kind
    else {
        unreachable!("called with non-mesh nodes")
    };
    let mismatches: Vec<String> = [
        (
            units == &m.units,
            format!("units '{units}' vs manifest '{}'", m.units),
        ),
        (up == &m.up, format!("up '{up}' vs manifest '{}'", m.up)),
        (
            (voxel - m.voxel).abs() <= 1e-9 * voxel.max(1.0),
            format!("voxel {voxel} vs manifest {}", m.voxel),
        ),
        (
            tris == &m.tris,
            format!("tris {tris} vs manifest {}", m.tris),
        ),
        (
            patches == &m.patches,
            format!("patches {patches} vs manifest {}", m.patches),
        ),
        (
            open_edges == &m.open_edges,
            format!("open edges {open_edges} vs manifest {}", m.open_edges),
        ),
    ]
    .into_iter()
    .filter(|(ok, _)| !ok)
    .map(|(_, msg)| msg)
    .collect();
    if !mismatches.is_empty() {
        return Err(format!(
            "mesh node '{}' (hash '{}') disagrees with the manifest — {}",
            n.id,
            m.hash,
            mismatches.join("; ")
        ));
    }
    Ok(())
}
