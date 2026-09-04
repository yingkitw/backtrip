use crate::decompile::DecompiledType;
use crate::error::{Error, Result};
use std::path::Path;

/// Write decompiled types to a directory, one file per type.
/// Returns the number of files written.
pub fn write_types(out_dir: &Path, types: &[DecompiledType]) -> Result<usize> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| Error::Io(e))?;
    let mut count = 0;
    for t in types {
        let path = out_dir.join(&t.file_name);
        std::fs::write(&path, &t.source).map_err(Error::Io)?;
        count += 1;
    }
    Ok(count)
}
