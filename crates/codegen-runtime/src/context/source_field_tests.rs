use crate::{
    Instance, InstanceKind, NamedInput, ScopeContext, SourcePathError, Value, field, group,
    integer, repeated, scalar,
};

fn details(value: Instance) -> Instance {
    group([field("Details", value)])
}

#[test]
fn absent_optional_group_shadows_another_item_only_for_source_fields() {
    let source = group([field(
        "Rows",
        repeated([
            details(group([field("Counter", scalar(integer(7)))])),
            group([]),
        ]),
    )]);
    let rows = ScopeContext::new(&source).walk_source(&["Rows"]);
    assert_eq!(
        rows[0].resolve_source_field(&["Rows", "Details", "Counter"]),
        Ok(integer(7))
    );
    assert_eq!(
        rows[1].resolve_source_field_in_frame(&["Rows"], &["Details", "Counter"]),
        Ok(Value::Null)
    );
    assert_eq!(
        rows[1].resolve_source_field(&["Rows", "Details", "Counter"]),
        Ok(Value::Null)
    );
    // Strict public APIs keep their original traversal and fallback contracts.
    assert_eq!(
        rows[1].resolve_scalar(&["Rows", "Details", "Counter"]),
        Ok(integer(7))
    );
    assert!(matches!(
        rows[1].resolve_scalar_in_frame(&["Rows"], &["Details", "Counter"]),
        Err(SourcePathError::MissingField { segment: 1, field, .. }) if field == "Details"
    ));
}

#[test]
fn null_intermediate_and_empty_optional_values_are_absence() {
    let source = group([field(
        "Rows",
        repeated([
            details(scalar(Value::Null)),
            details(group([])),
            details(repeated([])),
            details(scalar(Value::json_null())),
            details(group([field("Counter", scalar(Value::xml_nil()))])),
            details(group([field("Counter", scalar(Value::json_null()))])),
        ]),
    )]);
    let rows = ScopeContext::new(&source).walk_source(&["Rows"]);
    for row in &rows[..4] {
        assert_eq!(
            row.resolve_source_field_in_frame(&["Rows"], &["Details", "Counter"]),
            Ok(Value::Null)
        );
        assert_eq!(
            row.resolve_source_field(&["Rows", "Details", "Counter"]),
            Ok(Value::Null)
        );
    }
    assert!(matches!(
        rows[0].resolve_scalar_in_frame(&["Rows"], &["Details", "Counter"]),
        Err(SourcePathError::CannotTraverse {
            found: InstanceKind::Scalar,
            ..
        })
    ));
    assert_eq!(
        rows[4].resolve_source_field_in_frame(&["Rows"], &["Details", "Counter"]),
        Ok(Value::xml_nil())
    );
    assert_eq!(
        rows[5].resolve_source_field_in_frame(&["Rows"], &["Details", "Counter"]),
        Ok(Value::json_null())
    );
    assert!(matches!(
        rows[3].resolve_scalar_in_frame(&["Rows"], &["Details", "Counter"]),
        Err(SourcePathError::CannotTraverse {
            found: InstanceKind::Scalar,
            ..
        })
    ));
}

#[test]
fn source_field_absence_does_not_hide_bad_shapes_or_inactive_frames() {
    let source = group([field(
        "Rows",
        repeated([
            details(scalar(integer(9))),
            details(group([])),
            details(Instance::MappedSequence(vec![group([])])),
        ]),
    )]);
    let rows = ScopeContext::new(&source).walk_source(&["Rows"]);
    assert!(matches!(
        rows[0].resolve_source_field_in_frame(&["Rows"], &["Details", "Counter"]),
        Err(SourcePathError::CannotTraverse {
            found: InstanceKind::Scalar,
            segment: 2,
            ..
        })
    ));
    assert!(matches!(
        rows[0].resolve_source_field(&["Rows", "Details", "Counter"]),
        Err(SourcePathError::CannotTraverse {
            found: InstanceKind::Scalar,
            segment: 2,
            ..
        })
    ));
    assert!(matches!(
        rows[1].resolve_source_field_in_frame(&["Rows"], &["Details"]),
        Err(SourcePathError::ExpectedScalar {
            found: InstanceKind::Group,
            ..
        })
    ));
    assert!(matches!(
        rows[2].resolve_source_field_in_frame(&["Rows"], &["Details", "Counter"]),
        Err(SourcePathError::CannotTraverse {
            found: InstanceKind::MappedSequence,
            ..
        })
    ));
    assert_eq!(
        rows[1].resolve_source_field_in_frame(&["Inactive"], &["Details", "Counter"]),
        Err(SourcePathError::MissingFrame {
            frame: vec!["Inactive".into()],
            path: vec!["Details".into(), "Counter".into()],
        })
    );
}

#[test]
fn ordinary_relative_field_keeps_broadcast_while_absolute_owner_shadows_it() {
    let source = group([
        field("Settings", group([field("Counter", scalar(integer(99)))])),
        field("Rows", repeated([group([field("Settings", group([]))])])),
    ]);
    let rows = ScopeContext::new(&source).walk_source(&["Rows"]);
    assert_eq!(
        rows[0].resolve_source_field(&["Settings", "Counter"]),
        Ok(integer(99))
    );
    assert_eq!(
        rows[0].resolve_source_field(&["Rows", "Settings", "Counter"]),
        Ok(Value::Null)
    );
}

#[test]
fn named_and_root_optional_fields_are_null_without_changing_strict_paths() {
    let primary = group([
        field("Nullable", scalar(Value::Null)),
        field("ExplicitNull", scalar(Value::json_null())),
    ]);
    let settings = group([]);
    let inputs = [NamedInput {
        name: "Settings",
        instance: &settings,
    }];
    let context = ScopeContext::with_named_inputs(&primary, &inputs);
    assert_eq!(
        context.resolve_source_field(&["Settings", "Counter"]),
        Ok(Value::Null)
    );
    assert_eq!(
        context.resolve_source_field(&["Nullable", "Counter"]),
        Ok(Value::Null)
    );
    assert_eq!(
        context.resolve_source_field(&["ExplicitNull", "Counter"]),
        Ok(Value::Null)
    );
    assert_eq!(
        context.resolve_source_field(&["ExplicitNull"]),
        Ok(Value::json_null())
    );
    assert_eq!(
        context.resolve_source_field(&["Absent", "Counter"]),
        Ok(Value::Null)
    );
    assert!(matches!(
        context.resolve_scalar(&["Absent", "Counter"]),
        Err(SourcePathError::MissingField { .. })
    ));
}
