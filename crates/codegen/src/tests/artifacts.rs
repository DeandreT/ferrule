use crate::{ArtifactPath, ArtifactPathErrorKind, ArtifactSet, ArtifactSetError, GeneratedFile};

#[test]
fn paths_are_portable_relative_and_canonical() {
    let valid = ArtifactPath::new("src/generated/Grüße.rs").expect("UTF-8 paths are supported");
    assert_eq!(valid.as_str(), "src/generated/Grüße.rs");

    for (path, kind) in [
        ("", ArtifactPathErrorKind::Empty),
        ("/tmp/output.rs", ArtifactPathErrorKind::Absolute),
        ("C:/output.rs", ArtifactPathErrorKind::Absolute),
        ("../output.rs", ArtifactPathErrorKind::ParentComponent),
        ("src/../output.rs", ArtifactPathErrorKind::ParentComponent),
        ("./output.rs", ArtifactPathErrorKind::NonCanonicalComponent),
        (
            "src//output.rs",
            ArtifactPathErrorKind::NonCanonicalComponent,
        ),
        ("src\\output.rs", ArtifactPathErrorKind::Backslash),
        ("bad\0name", ArtifactPathErrorKind::NulByte),
    ] {
        assert_eq!(ArtifactPath::new(path).expect_err(path).kind, kind);
    }
}

#[test]
fn sets_sort_files_and_reject_duplicates() {
    let file = |path: &str, contents: &[u8]| {
        GeneratedFile::new(
            ArtifactPath::new(path).expect("test path is valid"),
            contents,
        )
    };
    let artifacts = ArtifactSet::new([
        file("z.txt", b"last"),
        file("nested/a.txt", b"middle"),
        file("a.txt", b"first"),
    ])
    .expect("paths are unique");

    assert_eq!(
        artifacts
            .files()
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec!["a.txt", "nested/a.txt", "z.txt"]
    );
    assert_eq!(artifacts.len(), 3);

    let duplicate = ArtifactPath::new("same.txt").expect("test path is valid");
    assert_eq!(
        ArtifactSet::new([
            GeneratedFile::new(duplicate.clone(), b"first"),
            GeneratedFile::new(duplicate.clone(), b"second"),
        ]),
        Err(ArtifactSetError::DuplicatePath(duplicate))
    );
}
