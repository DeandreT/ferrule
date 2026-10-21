//! Exact-owner object selection for owned dynamic reads.
use crate::{
    BasicSourceRoots, DeclaredNamedSourceRoot, Instance, OwnedSourceError, OwnedSourceIdentity,
    OwnedSourceLimits, RuntimeError, SourceOwnerRef, Value, field, group, repeated, scalar,
};

fn text(value: &str) -> Value {
    Value::String(value.into())
}
fn props(properties: &[(&str, Value)]) -> Instance {
    group(
        properties
            .iter()
            .map(|(name, value)| field(*name, scalar(value.clone()))),
    )
}
fn endpoint(root: &[(&str, Value)], rows: &[&[(&str, Value)]]) -> Instance {
    group([
        field("Props", props(root)),
        field(
            "Rows",
            repeated(rows.iter().map(|row| group([field("Props", props(row))]))),
        ),
    ])
}
fn limits() -> OwnedSourceLimits {
    OwnedSourceLimits {
        max_roots: 8,
        max_active_frames: 8,
        max_path_segments: 16,
        max_metadata_bytes: 65_536,
        max_walk_contexts: 64,
    }
}

#[test]
fn objects_and_properties_come_only_from_the_selected_owner_and_frame() {
    let primary = endpoint(
        &[("color", text("P.red")), ("blank", Value::json_null())],
        &[&[("color", text("P.row1"))], &[("size", text("P.row2"))]],
    );
    let lookup = endpoint(
        &[("size", text("N.small"))],
        &[&[("color", text("N.row1"))]],
    );
    let settings = props(&[("mode", text("S.fast"))]);
    let declared = [
        DeclaredNamedSourceRoot {
            name: "Lookup",
            instance: Some(&lookup),
        },
        DeclaredNamedSourceRoot {
            name: "Settings",
            instance: Some(&settings),
        },
        DeclaredNamedSourceRoot {
            name: "Missing",
            instance: None,
        },
    ];
    let roots = BasicSourceRoots::try_new(&primary, &declared, limits()).unwrap();
    let context = roots.root_context().unwrap();
    let lookup_owner = SourceOwnerRef::Named("Lookup");
    let read =
        |context: &crate::BasicScopeContext<'_>,
         owner,
         object: &[&str],
         frame: Option<&[&str]>,
         key: Value| { context.owned_dynamic_field(owner, object, frame, &key, true) };
    let props_path: &[&str] = &["Props"];
    assert_eq!(
        read(
            &context,
            SourceOwnerRef::Primary,
            props_path,
            None,
            text("color")
        ),
        Ok(text("P.red"))
    );
    // A property missing from the selected owner never borrows Primary's.
    assert_eq!(
        read(&context, lookup_owner, props_path, None, text("color")),
        Ok(Value::Null)
    );
    assert_eq!(
        read(&context, lookup_owner, props_path, None, text("size")),
        Ok(text("N.small"))
    );
    assert_eq!(
        read(
            &context,
            SourceOwnerRef::Primary,
            props_path,
            None,
            text("blank")
        ),
        Ok(Value::json_null())
    );
    // A non-string key reads nothing, including from an unsupplied owner.
    assert_eq!(
        read(
            &context,
            SourceOwnerRef::Named("Missing"),
            props_path,
            None,
            Value::Int(1)
        ),
        Ok(Value::Null)
    );
    assert_eq!(
        read(
            &context,
            SourceOwnerRef::Named("Settings"),
            &[],
            Some(&[]),
            text("mode")
        ),
        Ok(text("S.fast"))
    );
    assert_eq!(
        read(
            &context,
            SourceOwnerRef::Primary,
            props_path,
            Some(&[]),
            text("color")
        ),
        Ok(text("P.red"))
    );
    assert_eq!(
        read(
            &context,
            SourceOwnerRef::Named("Missing"),
            props_path,
            None,
            text("color")
        ),
        Err(RuntimeError::OwnedSource(
            OwnedSourceError::SourceUnavailable {
                name: "Missing".into()
            }
        ))
    );

    let row_props: &[&str] = &["Rows", "Props"];
    let rows = context
        .walk_owned_source(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    assert_eq!(
        read(
            rows[0].context(),
            SourceOwnerRef::Primary,
            row_props,
            None,
            text("color")
        ),
        Ok(text("P.row1"))
    );
    assert_eq!(
        read(
            rows[1].context(),
            SourceOwnerRef::Primary,
            row_props,
            Some(&["Rows"]),
            text("size")
        ),
        Ok(text("P.row2"))
    );
    // Lookup has no active row; Primary's row frame is never consulted.
    assert_eq!(
        read(
            rows[0].context(),
            lookup_owner,
            row_props,
            None,
            text("color")
        ),
        Err(RuntimeError::OwnedSource(
            OwnedSourceError::UniteratedRepetition {
                owner: OwnedSourceIdentity::Named("Lookup".into()),
                path: vec!["Rows".into(), "Props".into()],
                segment: 1,
            }
        ))
    );
    assert_eq!(
        read(
            rows[0].context(),
            lookup_owner,
            row_props,
            Some(&["Rows"]),
            text("color")
        ),
        Err(RuntimeError::OwnedSource(
            OwnedSourceError::InactiveSourceFrame {
                owner: OwnedSourceIdentity::Named("Lookup".into()),
                collection: Some(vec!["Rows".into()]),
            }
        ))
    );
    assert!(matches!(
        read(
            &context,
            SourceOwnerRef::Primary,
            props_path,
            Some(&["Rows"]),
            text("color")
        ),
        Err(RuntimeError::OwnedSource(
            OwnedSourceError::IncompatibleFramePath { .. }
        ))
    ));
}
