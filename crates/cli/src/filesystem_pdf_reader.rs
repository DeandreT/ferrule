//! Borrowed synchronous acquisition for configured filesystem PDF inputs.

use std::path::Path;

use ir::Instance;
use mapping::PdfLayout;

/// A caller-owned reader for configured filesystem PDF inputs.
///
/// The caller controls the implementation and its lifetime. Implementations need
/// neither `Send` nor `Sync`. This interface adds no global signal handler,
/// worker policy, implicit activation, or payload/Preview behavior.
pub trait FilesystemPdfReader {
    fn read_pdf(&self, path: &Path, layout: &PdfLayout) -> anyhow::Result<Instance>;
}
