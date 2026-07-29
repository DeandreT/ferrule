use super::*;
use crate::{NamedInput, Value, field, group, repeated, scalar};

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

#[test]
fn dynamic_fields_preserve_null_and_shadow_outward_objects() {
    let source = group([
        field(
            "Properties",
            group([
                field("wanted", scalar(text("outer"))),
                field("explicit-null", scalar(Value::json_null())),
                field("structural", group([])),
            ]),
        ),
        field(
            "Rows",
            repeated([
                group([field("Properties", group([]))]),
                group([field("Other", scalar(text("unrelated")))]),
            ]),
        ),
    ]);
    let context = ScopeContext::new(&source);

    assert_eq!(
        context.dynamic_scalar(None, &["Properties"], &text("wanted")),
        text("outer")
    );
    assert_eq!(
        context.dynamic_scalar(None, &["Properties"], &text("explicit-null")),
        Value::json_null()
    );
    assert_eq!(
        context.dynamic_scalar(None, &["Properties"], &text("structural")),
        Value::Null
    );
    assert_eq!(
        context.dynamic_scalar(None, &["Properties"], &text("missing")),
        Value::Null
    );
    assert_eq!(
        context.dynamic_scalar(None, &["Properties"], &Value::Int(1)),
        Value::Null
    );

    let rows = context.walk_source(&["Rows"]);
    assert_eq!(
        rows[0].dynamic_scalar(None, &["Properties"], &text("wanted")),
        Value::Null
    );
    assert_eq!(
        rows[1].dynamic_scalar(None, &["Properties"], &text("wanted")),
        text("outer")
    );
    assert_eq!(
        rows[1].dynamic_scalar(Some(&["Rows"]), &["Properties"], &text("wanted")),
        Value::Null
    );
}

#[test]
fn dynamic_fields_resolve_explicit_named_sources() {
    let source = group([]);
    let settings = group([
        field("selected", scalar(text("named"))),
        field("nil", scalar(Value::json_null())),
    ]);
    let inputs = [NamedInput {
        name: "Settings",
        instance: &settings,
    }];
    let context = ScopeContext::with_named_inputs(&source, &inputs);

    assert_eq!(
        context.dynamic_scalar(None, &["Settings"], &text("selected")),
        text("named")
    );
    assert_eq!(
        context.dynamic_scalar(None, &["Settings"], &text("nil")),
        Value::json_null()
    );
}
