use std::path::Path;

#[test]
fn committed_references_match_their_canonical_sources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let stale = reference_docs::stale(&root)
        .unwrap_or_else(|error| panic!("invalid reference source: {error}"));
    assert!(
        stale.is_empty(),
        "generated references are stale: {}; regenerate with `{}`",
        stale.join(", "),
        reference_docs::REGENERATE
    );
}
