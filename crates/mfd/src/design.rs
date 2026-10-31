//! One size budget and one entry-depth limit for ordinary and connected
//! design boundaries.

use std::fs::File;
use std::io::{Error, ErrorKind, Read};
use std::path::Path;

use crate::{MAX_MFD_DESIGN_BYTES, MAX_MFD_ENTRY_DEPTH, MfdError};

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

/// Parses a design for import. A design whose `<entry>` elements nest past
/// [`MAX_MFD_ENTRY_DEPTH`] is refused here, before any component is read.
pub(crate) fn parse(text: &str) -> Result<roxmltree::Document<'_>, MfdError> {
    let document = roxmltree::Document::parse(text)?;
    if entry_depth_exceeds(document.root(), MAX_MFD_ENTRY_DEPTH) {
        return Err(MfdError::UnsupportedImport(format!(
            "the .mfd design nests `<entry>` elements more than {MAX_MFD_ENTRY_DEPTH} levels deep"
        )));
    }
    Ok(document)
}

/// Whether any element below `root` has more than `limit` `<entry>`
/// ancestors-or-self. Counts without recursion and stops at the first excess.
fn entry_depth_exceeds(root: roxmltree::Node<'_, '_>, limit: usize) -> bool {
    let mut stack = vec![(root, 0usize)];
    while let Some((node, depth)) = stack.pop() {
        for child in node.children().filter(roxmltree::Node::is_element) {
            let depth = depth + usize::from(child.has_tag_name("entry"));
            if depth > limit {
                return true;
            }
            stack.push((child, depth));
        }
    }
    false
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
