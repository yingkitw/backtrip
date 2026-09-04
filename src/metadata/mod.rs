pub mod reader;
pub mod signatures;
pub mod streams;
pub mod tables;

pub use reader::Reader;
pub use tables::tbl;

use crate::error::Result;
use crate::pe::PeImage;

/// Load metadata from a parsed PE image.
pub fn load(pe: &PeImage) -> Result<(streams::MetadataRoot, tables::Tables)> {
    let cli = pe.cli_header()?;
    let off = pe.rva_to_offset(cli.metadata_rva)?;
    let meta_data = pe.slice_at_offset(off, cli.metadata_size as usize)?.to_vec();
    let root = streams::MetadataRoot::parse(meta_data)?;
    let tables = tables::parse_tables(&root)?;
    Ok((root, tables))
}
