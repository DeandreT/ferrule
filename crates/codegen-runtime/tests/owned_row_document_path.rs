use codegen_runtime::{
    BasicSourceRoots, DeclaredNamedSourceRoot, Instance, InstanceGroup, OwnedSourceError,
    OwnedSourceIdentity, OwnedSourceLimits, RuntimeError, SourceDocumentOrigin, SourceOwnerRef,
    SourceRoots, Value, field, group, repeated,
};

fn limits() -> OwnedSourceLimits {
    OwnedSourceLimits {
        max_roots: 8,
        max_active_frames: 16,
        max_path_segments: 16,
        max_metadata_bytes: 1_048_576,
        max_walk_contexts: 64,
    }
}
fn marked(value: Instance, path: &str) -> Instance {
    let Instance::Group(fields) = value else {
        panic!("group")
    };
    Instance::Group(
        fields.with_source_origin(
            SourceDocumentOrigin::new(path, None)
                .unwrap()
                .row(1)
                .unwrap(),
        ),
    )
}
fn fixtures() -> (Instance, Instance) {
    (
        marked(
            group([
                field("Ref", marked(group([]), "collision.csv")),
                field(
                    "Rows",
                    repeated([marked(
                        group([field(
                            "Children",
                            repeated([group([]), marked(group([]), "p.child.csv")]),
                        )]),
                        "p.row.csv",
                    )]),
                ),
            ]),
            "p.root.csv",
        ),
        marked(
            group([field("Rows", repeated([marked(group([]), "n.row.csv")]))]),
            "n.root.csv",
        ),
    )
}
fn text(path: &str) -> Value {
    Value::String(path.into())
}

#[test]
fn exact_owner_and_frame_never_borrow_missing_row_metadata() {
    let (primary, named) = fixtures();
    let declarations = [
        DeclaredNamedSourceRoot {
            name: "Ref",
            instance: Some(&named),
        },
        DeclaredNamedSourceRoot {
            name: "Absent",
            instance: None,
        },
    ];
    let roots = SourceRoots::try_new(&primary, &declarations, limits()).unwrap();
    let root = roots.root_context().unwrap();
    assert_eq!(
        root.owned_row_document_path(SourceOwnerRef::Primary, None),
        Ok(text("p.root.csv"))
    );
    assert_eq!(
        root.owned_row_document_path(SourceOwnerRef::Named("Ref"), None),
        Ok(text("n.root.csv"))
    );
    assert!(matches!(
        root.owned_row_document_path(SourceOwnerRef::Primary, Some(&["Rows"])),
        Err(OwnedSourceError::InactiveSourceFrame { .. })
    ));
    assert_eq!(
        root.owned_row_document_path(SourceOwnerRef::Named("Absent"), None),
        Err(OwnedSourceError::SourceUnavailable {
            name: "Absent".into()
        })
    );
    let primary_rows = root
        .walk_owned_source(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let named_rows = primary_rows[0]
        .walk_owned_source(SourceOwnerRef::Named("Ref"), &["Rows"])
        .unwrap();
    let both = &named_rows[0];
    assert_eq!(
        both.owned_row_document_path(SourceOwnerRef::Primary, None),
        Ok(text("p.row.csv"))
    );
    assert_eq!(
        both.owned_row_document_path(SourceOwnerRef::Named("Ref"), None),
        Ok(text("n.row.csv"))
    );
    assert_eq!(
        both.owned_row_document_path(SourceOwnerRef::Primary, Some(&[])),
        Ok(text("p.root.csv"))
    );
    assert_eq!(
        both.owned_row_document_path(SourceOwnerRef::Named("Ref"), Some(&[])),
        Ok(text("n.root.csv"))
    );
    let children = both
        .walk_owned_source(SourceOwnerRef::Primary, &["Rows", "Children"])
        .unwrap();
    assert_eq!(
        children[0].owned_row_document_path(SourceOwnerRef::Primary, None),
        Err(OwnedSourceError::MissingRowDocumentPath {
            owner: OwnedSourceIdentity::Primary,
            collection: None
        })
    );
    assert_eq!(
        children[0].owned_row_document_path(SourceOwnerRef::Primary, Some(&["Rows", "Children"])),
        Err(OwnedSourceError::MissingRowDocumentPath {
            owner: OwnedSourceIdentity::Primary,
            collection: Some(vec!["Rows".into(), "Children".into()])
        })
    );
    assert_eq!(
        children[0].owned_row_document_path(SourceOwnerRef::Primary, Some(&["Rows"])),
        Ok(text("p.row.csv"))
    );
    assert_eq!(
        children[0].owned_row_document_path(SourceOwnerRef::Named("Ref"), None),
        Ok(text("n.row.csv"))
    );
    assert_eq!(
        children[1].owned_row_document_path(SourceOwnerRef::Primary, None),
        Ok(text("p.child.csv"))
    );
}

#[test]
fn paired_context_keeps_the_same_selection_and_immutable_roots() {
    let (primary, named) = fixtures();
    let declarations = [DeclaredNamedSourceRoot {
        name: "Ref",
        instance: Some(&named),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declarations, limits()).unwrap();
    let root = roots.root_context().unwrap();
    let primary_rows = root
        .walk_owned_source(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let named_rows = primary_rows[0]
        .context()
        .walk_owned_source(SourceOwnerRef::Named("Ref"), &["Rows"])
        .unwrap();
    let both = named_rows[0].context();
    assert_eq!(
        both.owned_row_document_path(SourceOwnerRef::Primary, None),
        Ok(text("p.row.csv"))
    );
    assert_eq!(
        both.owned_row_document_path(SourceOwnerRef::Named("Ref"), None),
        Ok(text("n.row.csv"))
    );
    assert_eq!(
        both.owned_row_document_path(SourceOwnerRef::Primary, Some(&[])),
        Ok(text("p.root.csv"))
    );
    let children = both
        .walk_owned_source(SourceOwnerRef::Primary, &["Rows", "Children"])
        .unwrap();
    assert_eq!(
        children[0]
            .context()
            .owned_row_document_path(SourceOwnerRef::Primary, None),
        Err(RuntimeError::OwnedSource(
            OwnedSourceError::MissingRowDocumentPath {
                owner: OwnedSourceIdentity::Primary,
                collection: None
            }
        ))
    );
    assert_eq!(
        children[1]
            .context()
            .owned_row_document_path(SourceOwnerRef::Primary, None),
        Ok(text("p.child.csv"))
    );
}

#[test]
fn repeated_root_is_not_unwrapped_by_explicit_root_pin() {
    let primary = repeated([marked(Instance::Group(InstanceGroup::default()), "row.csv")]);
    let roots = SourceRoots::try_new(&primary, &[], limits()).unwrap();
    let root = roots.root_context().unwrap();
    assert!(matches!(
        root.owned_row_document_path(SourceOwnerRef::Primary, Some(&[])),
        Err(OwnedSourceError::MissingRowDocumentPath { .. })
    ));
    let rows = root
        .walk_owned_source(SourceOwnerRef::Primary, &[])
        .unwrap();
    assert_eq!(
        rows[0].owned_row_document_path(SourceOwnerRef::Primary, None),
        Ok(text("row.csv"))
    );
    assert!(matches!(
        rows[0].owned_row_document_path(SourceOwnerRef::Primary, Some(&[])),
        Err(OwnedSourceError::MissingRowDocumentPath { .. })
    ));
}

#[test]
fn query_path_limits_are_checked_before_metadata_selection() {
    let primary = marked(group([]), "root.csv");
    let roots = SourceRoots::try_new(
        &primary,
        &[],
        OwnedSourceLimits {
            max_path_segments: 0,
            ..limits()
        },
    )
    .unwrap();
    let root = roots.root_context().unwrap();
    assert_eq!(
        root.owned_row_document_path(SourceOwnerRef::Primary, None),
        Ok(text("root.csv"))
    );
    assert!(matches!(
        root.owned_row_document_path(SourceOwnerRef::Primary, Some(&["TooDeep"])),
        Err(OwnedSourceError::MetadataLimit {
            kind: codegen_runtime::OwnedSourceLimitKind::PathSegments,
            ..
        })
    ));
}
