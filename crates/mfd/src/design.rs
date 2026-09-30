//! One size budget for ordinary and connected design boundaries.

use std::fs::File;
use std::io::{Error, ErrorKind, Read};
use std::path::Path;

use crate::{MAX_MFD_DESIGN_BYTES, MfdError};

pub(crate) fn read(path: &Path) -> Result<String, MfdError> {
    let file = File::open(path)?;
    let size = file.metadata()?.len();
    if size > MAX_MFD_DESIGN_BYTES as u64 {
        return Err(import_size_error());
    }
    // Checking metadata alone would allow a growing file or a special file to
    // bypass the bound. Read at most one byte beyond it before decoding UTF-8.
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(MAX_MFD_DESIGN_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_MFD_DESIGN_BYTES {
        return Err(import_size_error());
    }
    String::from_utf8(bytes).map_err(|error| Error::new(ErrorKind::InvalidData, error).into())
}

fn import_size_error() -> MfdError {
    MfdError::UnsupportedImport("the .mfd design exceeds the 64 MiB byte limit".into())
}

pub(crate) fn validate_export(xml: &str) -> Result<(), MfdError> {
    if xml.len() > MAX_MFD_DESIGN_BYTES {
        Err(MfdError::Unsupported(
            "the rendered .mfd design exceeds the 64 MiB byte limit".into(),
        ))
    } else {
        Ok(())
    }
}
