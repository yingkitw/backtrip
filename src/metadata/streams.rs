use crate::error::{Error, Result};

/// Metadata root + stream headers.
#[derive(Debug)]
pub struct MetadataRoot {
    pub data: Vec<u8>,
    pub strings: HeapInfo,
    pub guid: HeapInfo,
    pub blob: HeapInfo,
    pub us: HeapInfo,
    pub tables: HeapInfo,
    pub version: String,
}

#[derive(Debug, Clone, Copy)]
pub struct HeapInfo {
    pub offset: usize,
    pub size: usize,
}

impl MetadataRoot {
    pub fn parse(data: Vec<u8>) -> Result<Self> {
        if data.len() < 16 {
            return Err(Error::InvalidMetadata("metadata root too small".into()));
        }
        let sig = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if sig != 0x424A_5342 {
            return Err(Error::InvalidMetadata(format!("bad metadata signature {sig:#x}")));
        }
        let version_len = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
        // version length is rounded up to a multiple of 4
        let version_padded = (version_len + 3) & !3;
        let version_start = 16;
        if version_start + version_padded > data.len() {
            return Err(Error::InvalidMetadata("version string out of bounds".into()));
        }
        let version = String::from_utf8_lossy(
            &data[version_start..version_start + version_len],
        )
        .trim_end_matches('\0')
        .to_string();

        let after_version = version_start + version_padded;
        if after_version + 4 > data.len() {
            return Err(Error::InvalidMetadata("stream headers missing".into()));
        }
        let flags = u16::from_le_bytes([data[after_version], data[after_version + 1]]);
        let _ = flags;
        let stream_count =
            u16::from_le_bytes([data[after_version + 2], data[after_version + 3]]) as usize;

        let mut strings = None;
        let mut guid = None;
        let mut blob = None;
        let mut us = None;
        let mut tables = None;

        let mut p = after_version + 4;
        for _ in 0..stream_count {
            if p + 8 > data.len() {
                return Err(Error::InvalidMetadata("stream header out of bounds".into()));
            }
            let offset = u32::from_le_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]) as usize;
            let size = u32::from_le_bytes([data[p + 4], data[p + 5], data[p + 6], data[p + 7]]) as usize;
            p += 8;
            // name: null-terminated, padded to 4 bytes
            let name_start = p;
            while p < data.len() && data[p] != 0 {
                p += 1;
            }
            let name = String::from_utf8_lossy(&data[name_start..p]).to_string();
            // skip the null terminator
            p += 1;
            // pad to 4-byte boundary relative to name_start
            let name_len = p - name_start;
            let padded = (name_len + 3) & !3;
            p = name_start + padded;

            let info = HeapInfo { offset, size };
            match name.as_str() {
                "#Strings" => strings = Some(info),
                "#GUID" => guid = Some(info),
                "#Blob" => blob = Some(info),
                "#US" => us = Some(info),
                "#~" | "#-" => tables = Some(info),
                _ => {}
            }
        }

        let empty = HeapInfo { offset: 0, size: 0 };
        Ok(MetadataRoot {
            data,
            strings: strings.unwrap_or(empty),
            guid: guid.unwrap_or(empty),
            blob: blob.unwrap_or(empty),
            us: us.unwrap_or(empty),
            tables: tables.ok_or_else(|| Error::InvalidMetadata("missing #~ stream".into()))?,
            version,
        })
    }

    fn heap_slice(&self, h: HeapInfo) -> &[u8] {
        let end = (h.offset + h.size).min(self.data.len());
        &self.data[h.offset..end]
    }

    pub fn strings_heap(&self) -> &[u8] {
        self.heap_slice(self.strings)
    }

    pub fn blob_heap(&self) -> &[u8] {
        self.heap_slice(self.blob)
    }

    pub fn tables_stream(&self) -> &[u8] {
        self.heap_slice(self.tables)
    }

    /// Read a null-terminated string from the #Strings heap at the given offset.
    pub fn get_string(&self, index: u32) -> Result<String> {
        if index == 0 {
            return Ok(String::new());
        }
        let heap = self.strings_heap();
        let i = index as usize;
        if i >= heap.len() {
            return Ok(String::new());
        }
        let end = heap[i..].iter().position(|&b| b == 0).map(|p| i + p).unwrap_or(heap.len());
        Ok(String::from_utf8_lossy(&heap[i..end]).to_string())
    }

    /// Read a blob from the #Blob heap at the given offset. Returns the blob
    /// bytes (after the length prefix). Index 0 = empty blob.
    pub fn get_blob(&self, index: u32) -> Result<&[u8]> {
        if index == 0 {
            return Ok(&[]);
        }
        let heap = self.blob_heap();
        let mut i = index as usize;
        if i >= heap.len() {
            return Err(Error::InvalidMetadata(format!("blob index {index} out of range")));
        }
        let (len, adv) = decode_compressed_uint(&heap[i..])?;
        i += adv;
        if i + len > heap.len() {
            return Err(Error::InvalidMetadata("blob length exceeds heap".into()));
        }
        Ok(&heap[i..i + len])
    }

    /// Read a user string from the #US heap. The blob is UTF-16LE; the final
    /// byte (a flag) is not part of the string.
    pub fn get_user_string(&self, index: u32) -> Result<String> {
        if index == 0 {
            return Ok(String::new());
        }
        let end = (self.us.offset + self.us.size).min(self.data.len());
        let heap = &self.data[self.us.offset..end];
        let i = index as usize;
        if i >= heap.len() {
            return Ok(String::new());
        }
        let (len, adv) = decode_compressed_uint(&heap[i..])?;
        let start = i + adv;
        if len == 0 || start + len > heap.len() {
            return Ok(String::new());
        }
        // The last byte is a flag; the string is the preceding bytes (UTF-16LE).
        let str_bytes = &heap[start..start + len - 1];
        let u16s: Vec<u16> = str_bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Ok(String::from_utf16_lossy(&u16s))
    }
}

/// Decode a ECMA-335 compressed unsigned integer. Returns (value, bytes_consumed).
pub fn decode_compressed_uint(data: &[u8]) -> Result<(usize, usize)> {
    if data.is_empty() {
        return Err(Error::InvalidMetadata("compressed uint: empty input".into()));
    }
    let b0 = data[0];
    if b0 & 0x80 == 0 {
        Ok((b0 as usize, 1))
    } else if b0 & 0xC0 == 0x80 {
        if data.len() < 2 {
            return Err(Error::InvalidMetadata("compressed uint: truncated 2-byte".into()));
        }
        let v = (((b0 & 0x3F) as usize) << 8) | data[1] as usize;
        Ok((v, 2))
    } else if b0 & 0xE0 == 0xC0 {
        if data.len() < 4 {
            return Err(Error::InvalidMetadata("compressed uint: truncated 4-byte".into()));
        }
        let v = (((b0 & 0x1F) as usize) << 24)
            | (data[1] as usize) << 16
            | (data[2] as usize) << 8
            | data[3] as usize;
        Ok((v, 4))
    } else {
        Err(Error::InvalidMetadata(format!("invalid compressed uint byte {b0:#x}")))
    }
}
