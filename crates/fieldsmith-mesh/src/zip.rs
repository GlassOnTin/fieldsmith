//! Minimal in-memory zip writing and reading (goal M6, step 7): the streaming DEFLATE
//! machinery [crate::export::TriMesh::write_3mf] uses, a whole-entry [`ZipWriter`] facade
//! for the `.fsmz` sidecar, and a reader that inflates and CRC-checks what either writes.
//!
//! Each entry's payload is DEFLATE-compressed in independent segments (pigz-style): every
//! segment is a fresh level-1 compressor ending on a sync flush, so it is a byte-aligned
//! run of non-final blocks, and segments concatenate into one valid raw stream that a final
//! empty block closes. Segments may be compressed in parallel and appended in index order,
//! so the bytes do not depend on the thread count.
//!
//! The reader accepts method 8 (deflate) and the EOCD layout this module writes: local
//! file headers, then the central directory immediately, then a 22-byte end record with no
//! archive comment. Anything else is refused with a named reason.

use miniz_oxide::deflate::core::{
    compress as deflate_compress, create_comp_flags_from_zip_params, CompressorOxide, TDEFLFlush,
    TDEFLStatus,
};

/// Compressed-space scratch for one `deflate` pass; any single emitted block fits well below.
const DEFLATE_BUF: usize = 512 * 1024;

fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Central-directory record of one entry, built when it finishes.
pub struct CentralEntry {
    pub(crate) name: String,
    pub(crate) offset: u32,
    pub(crate) crc: u32,
    pub(crate) comp: u32,
    pub(crate) uncomp: u32,
}

/// One independently compressed piece of an entry's payload: raw DEFLATE blocks ending on a
/// sync flush (byte-aligned, none final), plus the crc and length of the bytes it encodes.
pub struct Segment {
    pub(crate) z: Vec<u8>,
    pub(crate) crc: crc32fast::Hasher,
    pub(crate) len: usize,
}

fn level1() -> CompressorOxide {
    CompressorOxide::new(create_comp_flags_from_zip_params(1, 0, 0))
}

/// Compress `data` as one [`Segment`] with a fresh level-1 compressor.
pub fn deflate_segment(data: &[u8]) -> Segment {
    let mut crc = crc32fast::Hasher::default();
    crc.update(data);
    let mut d = level1();
    let mut buf = vec![0; DEFLATE_BUF];
    let mut z = Vec::with_capacity(data.len() / 4 + 64);
    let mut input = data;
    loop {
        let (status, used, made) = deflate_compress(&mut d, input, &mut buf, TDEFLFlush::Sync);
        assert_eq!(
            status,
            TDEFLStatus::Okay,
            "deflate segment failed: {status:?}"
        );
        z.extend_from_slice(&buf[..made]);
        input = &input[used..];
        // Done once the input is consumed and the flush fitted the scratch buffer.
        if input.is_empty() && made < buf.len() {
            break;
        }
        if used == 0 && made == 0 {
            panic!("deflate made no progress on {} bytes", input.len());
        }
    }
    Segment {
        z,
        crc,
        len: data.len(),
    }
}

/// One streamed zip entry. Segments append in order; [`ZipStream::finish`] closes the stream
/// with a final empty block and patches the local header's crc and size fields, so the sizes
/// never have to be known before the payload is written.
pub struct ZipStream<'a> {
    pub(crate) out: &'a mut Vec<u8>,
    hdr_at: usize,
    name: String,
    crc: crc32fast::Hasher,
    uncompressed: usize,
    compressed: usize,
}

impl<'a> ZipStream<'a> {
    pub fn start(out: &'a mut Vec<u8>, name: &str) -> Self {
        let hdr_at = out.len();
        push_u32(out, 0x04034b50);
        push_u16(out, 20); // version needed
        push_u16(out, 0); // flags
        push_u16(out, 8); // method: deflate
        push_u16(out, 0); // mod time
        push_u16(out, 0); // mod date (1980-01-01, the epoch zip pickers accept)
        push_u32(out, 0); // crc, patched in finish
        push_u32(out, 0); // compressed size, patched in finish
        push_u32(out, 0); // uncompressed size, patched in finish
        push_u16(out, name.len() as u16);
        push_u16(out, 0); // extra len
        out.extend_from_slice(name.as_bytes());
        Self {
            hdr_at,
            name: name.into(),
            crc: crc32fast::Hasher::default(),
            uncompressed: 0,
            compressed: 0,
            out,
        }
    }

    /// Append an already compressed segment.
    pub fn push_segment(&mut self, seg: Segment) {
        self.out.extend_from_slice(&seg.z);
        self.compressed += seg.z.len();
        self.crc.combine(&seg.crc);
        self.uncompressed += seg.len;
    }

    /// Compress and append a small payload piece as its own segment.
    pub fn push(&mut self, data: &[u8]) {
        if !data.is_empty() {
            self.push_segment(deflate_segment(data));
        }
    }

    /// End the payload: the final (empty) block, then patch the local header's crc and
    /// sizes, and return the central-directory record.
    pub fn finish(self) -> CentralEntry {
        let mut d = level1();
        let mut buf = vec![0; 64];
        let (status, _, made) = deflate_compress(&mut d, &[], &mut buf, TDEFLFlush::Finish);
        assert_eq!(
            status,
            TDEFLStatus::Done,
            "deflate stream did not end: {status:?}"
        );
        self.out.extend_from_slice(&buf[..made]);
        let compressed = self.compressed + made;
        let crc = self.crc.finalize();
        let hdr = &mut self.out[self.hdr_at..];
        hdr[14..18].copy_from_slice(&crc.to_le_bytes());
        hdr[18..22].copy_from_slice(&(compressed as u32).to_le_bytes());
        hdr[22..26].copy_from_slice(&(self.uncompressed as u32).to_le_bytes());
        CentralEntry {
            name: self.name,
            offset: self.hdr_at as u32,
            crc,
            comp: compressed as u32,
            uncomp: self.uncompressed as u32,
        }
    }
}

/// One in-memory payload through the streamed path (the two small package entries).
pub fn stream_entry(out: &mut Vec<u8>, name: &str, data: &[u8], records: &mut Vec<CentralEntry>) {
    let mut z = ZipStream::start(out, name);
    z.push(data);
    records.push(z.finish());
}

/// Central directory for the finished entries, then the package's end record.
pub fn write_central(out: &mut Vec<u8>, records: &[CentralEntry]) {
    let cd_offset = out.len() as u32;
    for e in records {
        push_u32(out, 0x02014b50);
        push_u16(out, 20); // version made by
        push_u16(out, 20); // version needed
        push_u16(out, 0); // flags
        push_u16(out, 8); // method
        push_u16(out, 0); // time
        push_u16(out, 0); // date
        push_u32(out, e.crc);
        push_u32(out, e.comp);
        push_u32(out, e.uncomp);
        push_u16(out, e.name.len() as u16);
        push_u16(out, 0); // extra
        push_u16(out, 0); // comment
        push_u16(out, 0); // disk start
        push_u16(out, 0); // internal attrs
        push_u32(out, 0); // external attrs
        push_u32(out, e.offset);
        out.extend_from_slice(e.name.as_bytes());
    }
    let cd_size = out.len() as u32 - cd_offset;
    push_u32(out, 0x06054b50);
    push_u16(out, 0);
    push_u16(out, 0);
    push_u16(out, records.len() as u16);
    push_u16(out, records.len() as u16);
    push_u32(out, cd_size);
    push_u32(out, cd_offset);
    push_u16(out, 0);
}

/// A one-shot zip archive: append whole-payload entries, then [`ZipWriter::finish`] closes
/// the central directory. The `.fsmz` sidecar writes with this; the 3MF's streamed model
/// entry uses [`ZipStream`] directly.
pub struct ZipWriter {
    out: Vec<u8>,
    records: Vec<CentralEntry>,
}

impl Default for ZipWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl ZipWriter {
    pub fn new() -> Self {
        Self {
            out: Vec::new(),
            records: Vec::new(),
        }
    }

    /// Append one entry, compressing `data` as a single DEFLATE segment.
    pub fn entry(&mut self, name: &str, data: &[u8]) {
        stream_entry(&mut self.out, name, data, &mut self.records);
    }

    /// Close the archive (central directory + end record) and return the bytes.
    pub fn finish(mut self) -> Vec<u8> {
        write_central(&mut self.out, &self.records);
        self.out
    }
}

/// One entry read back out of a zip: the inflated payload plus its name.
#[derive(Debug)]
pub struct ZipEntry {
    pub name: String,
    pub data: Vec<u8>,
}

/// CRC-32 (IEEE) of a payload — what local headers and the `.fsmz` manifest carry — so
/// sibling crates can verify payload digests without importing `crc32fast` themselves.
pub fn crc32(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}

/// Read an archive written by [`ZipWriter`] / [`ZipStream`]: inflate each local-file entry
/// checking its declared length and CRC, then reconcile the walked entry count and the
/// central-directory offset against the end record. Errors name the failing entry.
pub fn read_zip(bytes: &[u8]) -> Result<Vec<ZipEntry>, String> {
    let u32at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    let u16at = |i: usize| u16::from_le_bytes(bytes[i..i + 2].try_into().unwrap());
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 30 <= bytes.len() && u32at(i) == 0x04034b50 {
        let method = u16at(i + 8);
        let crc = u32at(i + 14);
        let csize = u32at(i + 18) as usize;
        let uncomp = u32at(i + 22) as usize;
        let nlen = u16at(i + 26) as usize;
        if i + 30 + nlen > bytes.len() {
            return Err("archive truncated inside an entry".into());
        }
        let name = String::from_utf8(bytes[i + 30..i + 30 + nlen].to_vec())
            .map_err(|_| "entry name is not UTF-8".to_string())?;
        if i + 30 + nlen + csize > bytes.len() {
            return Err(format!("archive truncated inside entry '{name}'"));
        }
        let raw = &bytes[i + 30 + nlen..i + 30 + nlen + csize];
        let data = match method {
            8 => miniz_oxide::inflate::decompress_to_vec(raw)
                .map_err(|e| format!("entry '{name}' does not inflate: {e}"))?,
            0 => raw[..uncomp].to_vec(),
            m => {
                return Err(format!(
                    "entry '{name}': method {m} is neither deflate (8) nor store (0)"
                ))
            }
        };
        if data.len() != uncomp {
            return Err(format!(
                "entry '{name}': header declares {uncomp} bytes, {got} inflated",
                got = data.len()
            ));
        }
        if crc32fast::hash(&data) != crc {
            return Err(format!("entry '{name}' fails its crc32"));
        }
        out.push(ZipEntry { name, data });
        i = i + 30 + nlen + csize;
    }
    // End record: 22 bytes at the tail (this module writes no archive comment or zip64
    // fields a general reader would have to locate).
    if bytes.len() < 22 || u32at(bytes.len() - 22) != 0x06054b50 {
        return Err(
            "not a recognised zip archive (missing the central directory end record)".into(),
        );
    }
    let e = bytes.len() - 22;
    let total = u16at(e + 10) as usize;
    if total != out.len() {
        return Err(format!(
            "the archive declares {total} entries, {walked} read",
            walked = out.len()
        ));
    }
    if u32at(e + 16) as usize != i {
        return Err("unexpected bytes between the entries and the central directory".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(entries: &[(&str, &[u8])]) -> Result<Vec<ZipEntry>, String> {
        let mut z = ZipWriter::new();
        for (name, data) in entries {
            z.entry(name, data);
        }
        read_zip(&z.finish())
    }

    #[test]
    fn entries_round_trip_with_crc_checks() {
        let got = round_trip(&[
            ("a.txt", b"hello world" as &[u8]),
            ("big.bin", &vec![7u8; (DEFLATE_BUF * 3 / 2).max(512)]),
            ("empty.txt", b""),
        ])
        .unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].name, "a.txt");
        assert_eq!(got[0].data, b"hello world");
        assert_eq!(got[1].name, "big.bin");
        assert_eq!(got[1].data.len(), (DEFLATE_BUF * 3 / 2).max(512));
        assert_eq!(got[2].name, "empty.txt");
        assert!(got[2].data.is_empty());
    }

    #[test]
    fn corrupt_crc_names_the_entry() {
        // The header's declared CRC no longer matches the payload: the entry inflates fine,
        // then the CRC check refuses it, naming it.
        let mut z = ZipWriter::new();
        z.entry("bad", &[0xAB; 200]);
        let mut bytes = z.finish();
        // Local header layout: sig(4) version(2) flags(2) method(2) time(2) date(2) crc(4)…
        // — the crc field sits at byte 14, and this is the archive's first entry.
        bytes[14] ^= 0x01;
        let err = read_zip(&bytes).unwrap_err();
        assert_eq!(err, "entry 'bad' fails its crc32");
    }

    #[test]
    fn truncated_end_record_is_refused() {
        let mut z = ZipWriter::new();
        z.entry("one", b"payload");
        let mut bytes = z.finish();
        bytes.truncate(bytes.len() - 22);
        assert_eq!(
            read_zip(&bytes).unwrap_err(),
            "not a recognised zip archive (missing the central directory end record)"
        );
    }

    #[test]
    fn truncated_payload_names_the_entry() {
        let mut z = ZipWriter::new();
        z.entry("one", b"payload");
        let mut z2 = ZipWriter::new();
        z2.entry("two", b"payload");
        let mut bytes = z2.finish();
        // Cut inside entry 'two's compressed stream (after its header and name).
        bytes.truncate(30 + 5 + 6);
        assert_eq!(
            read_zip(&bytes).unwrap_err(),
            "archive truncated inside entry 'two'"
        );
    }
}
