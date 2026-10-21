use crate::{
    BasicSourceRoots, DeclaredNamedSourceRoot, GeneratedItems, Instance, OwnedSourceError,
    OwnedSourceIdentity, OwnedSourceLimits, RuntimeError, SourceOwnerRef, Value, field, group,
    repeated, scalar, string,
};

fn limits() -> OwnedSourceLimits {
    OwnedSourceLimits {
        max_roots: 8,
        max_active_frames: 8,
        max_path_segments: 16,
        max_metadata_bytes: 65_536,
        max_walk_contexts: 32,
    }
}
fn endpoint(code: &str, rows: &[&str]) -> Instance {
    group([
        field("Code", scalar(string(code))),
        field(
            "Rows",
            repeated(
                rows.iter()
                    .map(|code| group([field("Code", scalar(string(*code)))])),
            ),
        ),
    ])
}

#[test]
fn items_append_a_lexical_frame_without_changing_exact_owned_reads() {
    let primary = endpoint("P", &["r1", "r2"]);
    let named = endpoint("N", &["n1"]);
    let declared = [DeclaredNamedSourceRoot {
        name: "Code",
        instance: Some(&named),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declared, limits()).unwrap();
    let root = roots.root_context().unwrap();
    let rows = root
        .walk_owned_source(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let row = rows[1].context();
    let items = GeneratedItems::new(vec![Value::Int(10), Value::String("x".into())]);
    let contexts = row
        .generated_item_contexts(&items)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(contexts.len(), 2);
    for (index, (context, item)) in contexts
        .iter()
        .zip([Value::Int(10), Value::String("x".into())])
        .enumerate()
    {
        assert_eq!(context.legacy_field(&[], None).unwrap(), item);
        assert_eq!(context.legacy_position(&[]), index + 1);
        assert_eq!(
            context
                .owned_field(SourceOwnerRef::Primary, &["Rows", "Code"], Some(&["Rows"]))
                .unwrap(),
            Value::String("r2".into())
        );
        assert_eq!(
            context
                .owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))
                .unwrap(),
            2
        );
        // Exact named and primary roots keep their colliding field values.
        assert_eq!(
            context
                .owned_field_with_root_pin(
                    SourceOwnerRef::Named("Code"),
                    &["Code"],
                    Some(&[]),
                    true
                )
                .unwrap(),
            Value::String("N".into())
        );
        assert_eq!(
            context
                .owned_field_with_root_pin(SourceOwnerRef::Primary, &["Code"], Some(&[]), true)
                .unwrap(),
            Value::String("P".into())
        );
        // An unpinned ordinary read still falls back outward past the item.
        assert_eq!(
            context.legacy_field(&["Code"], None).unwrap(),
            Value::String("r2".into())
        );
    }
    // The parent view is unchanged and still has its row position.
    assert_eq!(row.legacy_position(&[]), 2);
    assert_eq!(
        row.legacy_field(&["Code"], None).unwrap(),
        Value::String("r2".into())
    );
}

#[test]
fn an_empty_sequence_creates_no_view_and_missing_owners_fail_when_read() {
    let primary = endpoint("P", &[]);
    let declared = [DeclaredNamedSourceRoot {
        name: "Code",
        instance: None,
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declared, limits()).unwrap();
    let root = roots.root_context().unwrap();
    assert_eq!(
        root.generated_item_contexts(&GeneratedItems::new(vec![]))
            .count(),
        0
    );
    let items = GeneratedItems::new(vec![Value::Bool(true)]);
    let mut contexts = root.generated_item_contexts(&items);
    let context = contexts.next().unwrap().unwrap();
    assert!(contexts.next().is_none());
    assert_eq!(
        context.owned_field_with_root_pin(
            SourceOwnerRef::Named("Code"),
            &["Code"],
            Some(&[]),
            true
        ),
        Err(RuntimeError::OwnedSource(
            OwnedSourceError::SourceUnavailable {
                name: "Code".into()
            }
        ))
    );
    assert_eq!(
        context.owned_position(SourceOwnerRef::Primary, Some(&["Rows"])),
        Err(RuntimeError::OwnedSource(
            OwnedSourceError::InactiveSourceFrame {
                owner: OwnedSourceIdentity::Primary,
                collection: Some(vec!["Rows".into()]),
            }
        ))
    );
}
