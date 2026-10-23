//! The gitignored private compatibility corpus at `samples/ReferenceSamples`.
//! Tests that read it are ignored by ordinary runs; an explicit run without
//! it fails rather than reporting a pass.
use std::path::{Path, PathBuf};

/// The corpus root.
#[allow(dead_code)] // Not every test binary including this module reads the root.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples")
}

/// Fail unless every corpus input in `paths` is present.
#[track_caller]
pub fn require(paths: &[&Path]) {
    for path in paths {
        assert!(
            path.exists(),
            "private compatibility corpus not present at {}; this test needs the local \
             gitignored samples/ReferenceSamples corpus",
            path.display()
        );
    }
}
