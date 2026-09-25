//! `.jar` archive reading (zip format, PKWARE APPNOTE) with a built-in
//! DEFLATE decompressor (RFC 1951) — no external dependencies.
//!
//! Only reading is supported, and only the classic (non-Zip64) layout:
//! EOCD → central directory → local headers → entry data. Deflated
//! (method 8) and stored (method 0) entries cover essentially every jar
//! produced by `jar`, Maven, and Gradle.

use crate::error::{Error, Result};

/// One extracted archive entry (class files and manifests alike).
pub struct JarEntry {
    /// Entry name with `/` separators (e.g. `demo/Sample.class`).
    pub name: String,
    pub data: Vec<u8>,
}

/// Read every entry of a zip archive into memory.
pub fn read_entries(data: &[u8]) -> Result<Vec<JarEntry>> {
    let eocd = find_eocd(data)?;
    let r = &mut Reader { data, pos: eocd };
    let sig = r.u32()?;
    if sig != 0x0605_4B50 {
        return Err(Error::InvalidClassFile(
            "bad zip: end-of-central-directory signature".into(),
        ));
    }
    let disk = r.u16()?;
    let cd_disk = r.u16()?;
    let _disk_entries = r.u16()?;
    let total_entries = r.u16()?;
    let _cd_size = r.u32()?;
    let cd_offset = r.u32()? as usize;
    if disk != 0 || cd_disk != 0 {
        return Err(Error::NotImplemented(
            "multi-disk zip archives are not supported",
        ));
    }
    if total_entries as u32 == 0xFFFF {
        return Err(Error::NotImplemented("zip64 archives are not supported"));
    }

    let mut entries = Vec::with_capacity(total_entries as usize);
    let cd = &mut Reader { data, pos: cd_offset };
    for _ in 0..total_entries {
        if cd.u32()? != 0x0201_4B50 {
            return Err(Error::InvalidClassFile(
                "bad zip: central-directory signature".into(),
            ));
        }
        let _version_made = cd.u16()?;
        let _version_needed = cd.u16()?;
        let _flags = cd.u16()?;
        let method = cd.u16()?;
        let _mtime = cd.u16()?;
        let _mdate = cd.u16()?;
        let _crc32 = cd.u32()?;
        let compressed_size = cd.u32()? as usize;
        let _uncompressed_size = cd.u32()? as usize;
        let name_len = cd.u16()? as usize;
        let extra_len = cd.u16()? as usize;
        let comment_len = cd.u16()? as usize;
        let _disk_start = cd.u16()?;
        let _internal_attrs = cd.u16()?;
        let _external_attrs = cd.u32()?;
        let local_offset = cd.u32()? as usize;
        let name = cd.bytes(name_len)?.to_vec();
        cd.skip(extra_len + comment_len)?;
        let name = String::from_utf8_lossy(&name).into_owned();
        if name.ends_with('/') {
            continue; // directory entry
        }
        let payload = read_entry_data(data, local_offset, method, compressed_size)?;
        entries.push(JarEntry { name, data: payload });
    }
    Ok(entries)
}

/// Parse one local file header and return its (decompressed) data.
fn read_entry_data(
    data: &[u8],
    offset: usize,
    method: u16,
    compressed_size: usize,
) -> Result<Vec<u8>> {
    let r = &mut Reader { data, pos: offset };
    if r.u32()? != 0x0403_4B50 {
        return Err(Error::InvalidClassFile(
            "bad zip: local-header signature".into(),
        ));
    }
    let _version = r.u16()?;
    let _flags = r.u16()?;
    r.skip(2 + 2 + 2 + 4 + 4 + 4)?; // method/time/date/crc/csize/usize (from central dir)
    let name_len = r.u16()? as usize;
    let extra_len = r.u16()? as usize;
    r.skip(name_len + extra_len)?;
    let start = r.pos;
    // Size-from-central-dir is authoritative; flag bit 3 (streaming) is not
    // produced for jar class entries by jar/Maven/Gradle.
    let raw = read_slice(data, start, compressed_size)?;
    match method {
        0 => Ok(raw.to_vec()),
        8 => inflate(raw),
        other => Err(Error::InvalidClassFile(format!(
            "unsupported zip compression method {other} for archive entry"
        ))),
    }
}

/// Locate the End Of Central Directory record (scan backwards past a
/// possible archive comment, max 65535 bytes + fixed 22).
fn find_eocd(data: &[u8]) -> Result<usize> {
    if data.len() < 22 {
        return Err(Error::InvalidClassFile("file too small for a zip".into()));
    }
    let min_start = data.len().saturating_sub(22 + 65_535);
    let mut i = data.len() - 22;
    loop {
        if data[i..i + 4] == [0x50, 0x4B, 0x05, 0x06] {
            return Ok(i);
        }
        if i == min_start {
            return Err(Error::InvalidClassFile(
                "bad zip: end-of-central-directory not found".into(),
            ));
        }
        i -= 1;
    }
}

// ---- raw DEFLATE (RFC 1951), puff-style -------------------------------------

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
}

impl BitReader<'_> {
    fn bits(&mut self, need: u32) -> Result<u32> {
        let mut out = 0u32;
        for i in 0..need {
            let byte = *self
                .data
                .get(self.pos)
                .ok_or_else(|| Error::InvalidClassFile("deflate: truncated".into()))?;
            let b = (byte >> self.bit) & 1;
            out |= (b as u32) << i;
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.pos += 1;
            }
        }
        Ok(out)
    }
    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.pos += 1;
        }
    }
}

/// Canonical Huffman decoding table (zlib "puff" style).
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn build(lengths: &[u8]) -> Result<Huffman> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[l as usize] += 1;
        }
        counts[0] = 0;
        let mut offs = [0u16; 16];
        for len in 1..16 {
            offs[len] = offs[len - 1] + counts[len - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        Ok(Huffman { counts, symbols })
    }

    fn decode(&self, br: &mut BitReader) -> Result<u16> {
        let mut code = 0isize;
        let mut first = 0isize;
        let mut index = 0isize;
        for len in 1..16 {
            code |= br.bits(1)? as isize;
            let count = self.counts[len] as isize;
            if code - first < count {
                return self
                    .symbols
                    .get((index + (code - first)) as usize)
                    .copied()
                    .ok_or_else(|| Error::InvalidClassFile("deflate: bad symbol".into()));
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(Error::InvalidClassFile(
            "deflate: ran out of codes".into(),
        ))
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
    131, 163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
    13, 13,
];
const CODE_LENGTH_ORDER: [usize; 19] =
    [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// Decompress a raw DEFLATE stream.
pub fn inflate(data: &[u8]) -> Result<Vec<u8>> {
    let br = &mut BitReader { data, pos: 0, bit: 0 };
    let mut out: Vec<u8> = Vec::with_capacity(data.len() * 4);
    loop {
        let last = br.bits(1)?;
        match br.bits(2)? {
            0 => {
                // stored block
                br.align();
                if br.pos + 4 > data.len() {
                    return Err(Error::InvalidClassFile("deflate: short stored block".into()));
                }
                let len = u16::from_le_bytes([data[br.pos], data[br.pos + 1]]) as usize;
                br.pos += 4; // LEN + NLEN (NLEN not verified — matches puff's slack)
                let end = br.pos + len;
                if end > data.len() {
                    return Err(Error::InvalidClassFile("deflate: stored overflow".into()));
                }
                out.extend_from_slice(&data[br.pos..end]);
                br.pos = end;
            }
            1 => {
                let mut lit_lengths = [8u8; 288];
                for l in lit_lengths[144..256].iter_mut() {
                    *l = 9;
                }
                for l in lit_lengths[256..280].iter_mut() {
                    *l = 7;
                }
                let dist_lengths = [5u8; 30];
                let lit = Huffman::build(&lit_lengths)?;
                let dist = Huffman::build(&dist_lengths)?;
                inflate_block(br, &lit, &dist, &mut out)?;
            }
            2 => {
                let hlit = br.bits(5)? as usize + 257;
                let hdist = br.bits(5)? as usize + 1;
                let hclen = br.bits(4)? as usize + 4;
                let mut cl_lengths = [0u8; 19];
                for i in 0..hclen {
                    cl_lengths[CODE_LENGTH_ORDER[i]] = br.bits(3)? as u8;
                }
                let cl = Huffman::build(&cl_lengths)?;
                let mut lengths = vec![0u8; hlit + hdist];
                let mut i = 0;
                while i < lengths.len() {
                    let sym = cl.decode(br)?;
                    match sym {
                        0..=15 => {
                            lengths[i] = sym as u8;
                            i += 1;
                        }
                        16 => {
                            if i == 0 {
                                return Err(Error::InvalidClassFile(
                                    "deflate: repeat with no previous length".into(),
                                ));
                            }
                            let prev = lengths[i - 1];
                            let n = 3 + br.bits(2)? as usize;
                            for _ in 0..n {
                                if i >= lengths.len() {
                                    return Err(Error::InvalidClassFile(
                                        "deflate: repeat overflow".into(),
                                    ));
                                }
                                lengths[i] = prev;
                                i += 1;
                            }
                        }
                        17 => {
                            let n = 3 + br.bits(3)? as usize;
                            i += n;
                        }
                        18 => {
                            let n = 11 + br.bits(7)? as usize;
                            i += n;
                        }
                        _ => {
                            return Err(Error::InvalidClassFile(
                                "deflate: bad code-length symbol".into(),
                            ))
                        }
                    }
                }
                if i > lengths.len() {
                    return Err(Error::InvalidClassFile(
                        "deflate: code lengths overflow".into(),
                    ));
                }
                let lit = Huffman::build(&lengths[..hlit])?;
                let dist = Huffman::build(&lengths[hlit..])?;
                inflate_block(br, &lit, &dist, &mut out)?;
            }
            _ => return Err(Error::InvalidClassFile("deflate: bad block type".into())),
        }
        if last == 1 {
            break;
        }
    }
    Ok(out)
}

/// Decode one Huffman-compressed block into `out` (LZ77 window = `out`).
fn inflate_block(
    br: &mut BitReader,
    lit: &Huffman,
    dist: &Huffman,
    out: &mut Vec<u8>,
) -> Result<()> {
    loop {
        let sym = lit.decode(br)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Ok(()),
            257..=285 => {
                let idx = (sym - 257) as usize;
                let len = LENGTH_BASE[idx] as usize + br.bits(LENGTH_EXTRA[idx] as u32)? as usize;
                let dsym = dist.decode(br)? as usize;
                if dsym >= 30 {
                    return Err(Error::InvalidClassFile("deflate: bad distance".into()));
                }
                let d =
                    DIST_BASE[dsym] as usize + br.bits(DIST_EXTRA[dsym] as u32)? as usize;
                if d > out.len() {
                    return Err(Error::InvalidClassFile(
                        "deflate: distance beyond output".into(),
                    ));
                }
                let start = out.len() - d;
                for k in 0..len {
                    let b = out[start + k];
                    out.push(b);
                }
            }
            _ => return Err(Error::InvalidClassFile("deflate: bad literal/length".into())),
        }
    }
}

// ---- small reader -------------------------------------------------------------

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn u16(&mut self) -> Result<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n)?;
        Ok(())
    }
    fn bytes(&mut self, n: usize) -> Result<&[u8]> {
        read_slice(self.data, self.pos, n).inspect(|_| {
            self.pos += n;
        })
    }
}

fn read_slice(data: &[u8], pos: usize, n: usize) -> Result<&[u8]> {
    data.get(pos..pos + n).ok_or_else(|| {
        Error::InvalidClassFile(format!(
            "zip: need {n} bytes at offset {pos} (file is {} bytes)",
            data.len()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVEL1: &[u8] = &[
        0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0xc0, 0x20, 0xcb, 0xf3, 0x8b, 0x72, 0x52, 0x0,
    ];
    const LEVEL1_PLAIN: &[u8] = &[
        0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x68, 0x65,
        0x6c, 0x6c, 0x6f, 0x20, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x77, 0x6f, 0x72, 0x6c,
        0x64,
    ];
    const LEVEL9: &[u8] = &[
        0x2b, 0xc9, 0x48, 0x55, 0x28, 0x2c, 0xcd, 0x4c, 0xce, 0x56, 0x48, 0x2a, 0xca, 0x2f,
        0xcf, 0x53, 0x48, 0xcb, 0xaf, 0x50, 0xc8, 0x2a, 0xcd, 0x2d, 0x28, 0x56, 0xc8, 0x2f,
        0x4b, 0x2d, 0x52, 0x28, 0x1, 0x4a, 0xe7, 0x24, 0x56, 0x55, 0x2a, 0xa4, 0xe4, 0xa7,
        0x83, 0x39, 0x3, 0xad, 0x16, 0x0,
    ];
    const TINY: &[u8] = &[0x4b, 0x4, 0x0];

    #[test]
    fn inflate_fixed_huffman() {
        assert_eq!(inflate(LEVEL1).unwrap(), LEVEL1_PLAIN);
    }

    #[test]
    fn inflate_dynamic_huffman() {
        let plain = b"the quick brown fox jumps over the lazy dog ".repeat(4);
        assert_eq!(inflate(LEVEL9).unwrap(), plain);
    }

    #[test]
    fn inflate_tiny() {
        assert_eq!(inflate(TINY).unwrap(), b"a");
    }

    #[test]
    fn inflate_stored_block() {
        // manual stored block: final=1, type=00, align, LEN=3, NLEN, data
        let data = [0x01, 0x03, 0x00, 0xFC, 0xFF, 0x61, 0x62, 0x63];
        assert_eq!(inflate(&data).unwrap(), b"abc");
    }

    /// Build a minimal stored-method zip in memory: local header + data +
    /// central directory + EOCD.
    fn build_stored_zip(name: &[u8], contents: &[u8]) -> Vec<u8> {
        let crc = 0u32; // content-agnostic for our reader
        let mut z = Vec::new();
        let local_offset = 0usize;
        // local file header
        z.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
        z.extend_from_slice(&20u16.to_le_bytes()); // version
        z.extend_from_slice(&0u16.to_le_bytes()); // flags
        z.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        z.extend_from_slice(&0u16.to_le_bytes()); // time
        z.extend_from_slice(&0u16.to_le_bytes()); // date
        z.extend_from_slice(&crc.to_le_bytes());
        z.extend_from_slice(&(contents.len() as u32).to_le_bytes());
        z.extend_from_slice(&(contents.len() as u32).to_le_bytes());
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes()); // extra len
        z.extend_from_slice(name);
        z.extend_from_slice(contents);
        // central directory
        let cd_offset = z.len() as u32;
        z.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
        z.extend_from_slice(&20u16.to_le_bytes());
        z.extend_from_slice(&20u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes()); // method
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&crc.to_le_bytes());
        z.extend_from_slice(&(contents.len() as u32).to_le_bytes());
        z.extend_from_slice(&(contents.len() as u32).to_le_bytes());
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes()); // extra
        z.extend_from_slice(&0u16.to_le_bytes()); // comment
        z.extend_from_slice(&0u16.to_le_bytes()); // disk start
        z.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        z.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        z.extend_from_slice(&(local_offset as u32).to_le_bytes());
        z.extend_from_slice(name);
        let cd_size = z.len() as u32 - cd_offset;
        // EOCD
        z.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes()); // disk
        z.extend_from_slice(&0u16.to_le_bytes()); // cd disk
        z.extend_from_slice(&1u16.to_le_bytes()); // disk entries
        z.extend_from_slice(&1u16.to_le_bytes()); // total
        z.extend_from_slice(&cd_size.to_le_bytes());
        z.extend_from_slice(&cd_offset.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes()); // comment len
        z
    }

    #[test]
    fn reads_stored_zip_entries() {
        let zip = build_stored_zip(b"demo/Hello.class", b"CAFEBABE");
        let entries = read_entries(&zip).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "demo/Hello.class");
        assert_eq!(entries[0].data, b"CAFEBABE");
    }

    #[test]
    fn skips_directory_entries() {
        let zip = build_stored_zip(b"demo/", b"");
        assert!(read_entries(&zip).unwrap().is_empty());
    }

    #[test]
    fn rejects_non_zip() {
        assert!(read_entries(b"not a zip at all, definitely not").is_err());
    }
}
