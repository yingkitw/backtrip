use crate::error::{Error, Result};

/// A minimal PE parser that locates the .NET CLI header and metadata.
/// Only parses what's needed to reach the managed metadata.

#[derive(Debug, Clone, Copy)]
pub struct SectionHeader {
    pub virtual_address: u32,
    pub virtual_size: u32,
    pub pointer_to_raw_data: u32,
    pub size_of_raw_data: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct DataDirectory {
    pub virtual_address: u32,
    pub size: u32,
}

#[derive(Debug)]
pub struct PeImage {
    pub data: Vec<u8>,
    pub sections: Vec<SectionHeader>,
    pub cli_header_rva: u32,
    pub cli_header_size: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct CliHeader {
    pub metadata_rva: u32,
    pub metadata_size: u32,
    pub entry_point_token: u32,
}

impl PeImage {
    pub fn parse(data: Vec<u8>) -> Result<Self> {
        if data.len() < 0x40 {
            return Err(Error::InvalidPe("file too small for DOS header".into()));
        }
        if &data[0..2] != b"MZ" {
            return Err(Error::InvalidPe("missing MZ signature".into()));
        }
        let e_lfanew = u32::from_le_bytes([data[0x3c], data[0x3d], data[0x3e], data[0x3f]]) as usize;
        if e_lfanew + 24 > data.len() {
            return Err(Error::InvalidPe("invalid e_lfanew".into()));
        }
        if &data[e_lfanew..e_lfanew + 4] != b"PE\0\0" {
            return Err(Error::InvalidPe("missing PE signature".into()));
        }
        let coff = e_lfanew + 4;
        let number_of_sections =
            u16::from_le_bytes([data[coff + 2], data[coff + 3]]) as usize;
        let size_of_optional_header =
            u16::from_le_bytes([data[coff + 16], data[coff + 17]]) as usize;
        let opt = coff + 20;
        if opt + size_of_optional_header > data.len() {
            return Err(Error::InvalidPe("optional header out of bounds".into()));
        }
        let magic = u16::from_le_bytes([data[opt], data[opt + 1]]);
        let data_dirs_offset: usize = match magic {
            0x10b => opt + 96,       // PE32
            0x20b => opt + 112,      // PE32+
            _ => return Err(Error::InvalidPe(format!("unknown optional header magic {magic:#x}"))),
        };

        // CLI runtime header is data directory index 14.
        let cli_dir = data_dirs_offset + 14 * 8;
        if cli_dir + 8 > data.len() {
            return Err(Error::InvalidPe("CLI data directory out of bounds".into()));
        }
        let cli_header_rva = u32::from_le_bytes([
            data[cli_dir],
            data[cli_dir + 1],
            data[cli_dir + 2],
            data[cli_dir + 3],
        ]);
        let cli_header_size = u32::from_le_bytes([
            data[cli_dir + 4],
            data[cli_dir + 5],
            data[cli_dir + 6],
            data[cli_dir + 7],
        ]);
        if cli_header_rva == 0 {
            return Err(Error::InvalidPe(
                "no CLI runtime header; not a managed .NET assembly".into(),
            ));
        }

        // Section headers follow the optional header.
        let sections_off = opt + size_of_optional_header;
        let mut sections = Vec::with_capacity(number_of_sections);
        for i in 0..number_of_sections {
            let s = sections_off + i * 40;
            if s + 40 > data.len() {
                return Err(Error::InvalidPe("section header out of bounds".into()));
            }
            let virtual_size = u32::from_le_bytes([data[s + 8], data[s + 9], data[s + 10], data[s + 11]]);
            let virtual_address =
                u32::from_le_bytes([data[s + 12], data[s + 13], data[s + 14], data[s + 15]]);
            let size_of_raw_data =
                u32::from_le_bytes([data[s + 16], data[s + 17], data[s + 18], data[s + 19]]);
            let pointer_to_raw_data =
                u32::from_le_bytes([data[s + 20], data[s + 21], data[s + 22], data[s + 23]]);
            sections.push(SectionHeader {
                virtual_address,
                virtual_size,
                pointer_to_raw_data,
                size_of_raw_data,
            });
        }

        Ok(PeImage {
            data,
            sections,
            cli_header_rva,
            cli_header_size,
        })
    }

    /// Convert a Relative Virtual Address to a file offset.
    pub fn rva_to_offset(&self, rva: u32) -> Result<usize> {
        for s in &self.sections {
            let va = s.virtual_address;
            let vsize = if s.virtual_size != 0 {
                s.virtual_size
            } else {
                s.size_of_raw_data
            };
            if rva >= va && rva < va + vsize {
                let delta = rva - va;
                // Clamp to raw data size to avoid reading past section data.
                let raw = s.pointer_to_raw_data + delta.min(s.size_of_raw_data);
                return Ok(raw as usize);
            }
        }
        Err(Error::InvalidPe(format!("could not resolve RVA {rva:#x}")))
    }

    pub fn cli_header(&self) -> Result<CliHeader> {
        let off = self.rva_to_offset(self.cli_header_rva)?;
        let d = &self.data;
        if off + 72 > d.len() {
            return Err(Error::InvalidPe("CLI header out of bounds".into()));
        }
        // IMAGE_COR20_HEADER layout (II.25.3.3):
        // 0  cb (u32)
        // 4  MajorRuntimeVersion (u16)
        // 6  MinorRuntimeVersion (u16)
        // 8  MetaData rva (u32)   <- IMAGE_DATA_DIRECTORY
        // 12 MetaData size (u32)
        // 16 Flags (u32)
        // 20 EntryPointToken (u32)
        let metadata_rva = u32::from_le_bytes([d[off + 8], d[off + 9], d[off + 10], d[off + 11]]);
        let metadata_size = u32::from_le_bytes([d[off + 12], d[off + 13], d[off + 14], d[off + 15]]);
        let entry_point_token = u32::from_le_bytes([d[off + 20], d[off + 21], d[off + 22], d[off + 23]]);
        Ok(CliHeader {
            metadata_rva,
            metadata_size,
            entry_point_token,
        })
    }

    pub fn slice_at_offset(&self, off: usize, len: usize) -> Result<&[u8]> {
        if off + len > self.data.len() {
            return Err(Error::InvalidPe(format!("slice out of bounds off={off} len={len}")));
        }
        Ok(&self.data[off..off + len])
    }
}
