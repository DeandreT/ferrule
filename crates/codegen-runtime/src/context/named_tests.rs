use std::path::Path;

use crate::{
    ExecutionContext, GeneratedItems, Instance, NamedInput, RecursiveCollectPaths, RuntimeValue,
    ScopeContext, Value, field, group, recursive_collect, repeated, scalar,
};

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

fn catalog_row(key: i64, value: &str) -> Instance {
    group([
        field("Key", scalar(Value::Int(key))),
        field("Value", scalar(text(value))),
    ])
}

#[test]
fn named_inputs_are_an_outer_fallback_for_every_collection_operation() {
    let source = group([field(
        "Orders",
        repeated([group([field("Customer", scalar(Value::Int(2)))])]),
    )]);
    let catalog = repeated([catalog_row(1, "one"), catalog_row(2, "two")]);
    let settings = group([field("Label", scalar(text("configured")))]);
    let inputs = [
        NamedInput {
            name: "Catalog",
            instance: &catalog,
        },
        NamedInput {
            name: "Settings",
            instance: &settings,
        },
    ];
    let context = ScopeContext::with_named_inputs(&source, &inputs);

    assert_eq!(
        context.resolve_scalar(&["Settings", "Label"]),
        Ok(text("configured"))
    );

    let rows = context.walk_source(&["Catalog"]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].resolve_scalar(&["Key"]), Ok(Value::Int(1)));
    assert_eq!(rows[1].resolve_scalar(&["Key"]), Ok(Value::Int(2)));
    assert_eq!(rows[1].position(&["Catalog"]), 2);
    assert_eq!(
        rows[1].resolve_scalar_in_frame(&["Catalog"], &["Value"]),
        Ok(text("two"))
    );

    let aggregate = context.aggregate_items(&["Catalog"]);
    assert_eq!(aggregate.len(), 2);
    assert_eq!(
        aggregate[0].aggregate_current_scalar(&["Value"]),
        text("one")
    );
    assert_eq!(
        context.lookup(&["Catalog"], &["Key"], &Value::Int(2), &["Value"]),
        Ok(text("two"))
    );
}

#[test]
fn active_and_primary_owners_precede_named_inputs_without_hiding_lookup_fallback() {
    let source = group([
        field("Catalog", group([field("Label", scalar(text("primary")))])),
        field(
            "Rows",
            repeated([group([field(
                "Catalog",
                group([field("Label", scalar(text("active")))]),
            )])]),
        ),
    ]);
    let catalog = repeated([catalog_row(1, "named")]);
    let inputs = [NamedInput {
        name: "Catalog",
        instance: &catalog,
    }];
    let context = ScopeContext::with_named_inputs(&source, &inputs);

    assert_eq!(
        context.resolve_scalar(&["Catalog", "Label"]),
        Ok(text("primary"))
    );
    let active = context.walk_source(&["Rows"]);
    assert_eq!(
        active[0].resolve_scalar(&["Catalog", "Label"]),
        Ok(text("active"))
    );

    // Iteration stops at the primary owner, even though the named value is repeated.
    let selected = context.walk_source(&["Catalog"]);
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].resolve_scalar(&["Label"]), Ok(text("primary")));

    // Lookup instead continues outward until the terminal value is repeated.
    assert_eq!(
        context.lookup(&["Catalog"], &["Key"], &Value::Int(1), &["Value"]),
        Ok(text("named"))
    );
}

#[test]
fn recursive_generated_and_execution_contexts_retain_named_inputs() {
    let leaf = |name: &str| group([field("Name", scalar(text(name)))]);
    let node = |name: &str, files: Vec<Instance>, children: Vec<Instance>| {
        group([
            field("Name", scalar(text(name))),
            field("Files", repeated(files)),
            field("Children", repeated(children)),
        ])
    };
    let tree = repeated([node(
        "root",
        vec![leaf("top.txt")],
        vec![node("child", vec![leaf("nested.txt")], Vec::new())],
    )]);
    let source = group([]);
    let inputs = [NamedInput {
        name: "Tree",
        instance: &tree,
    }];
    let execution = ExecutionContext::new(Path::new("map.ferrule.json"));
    let context =
        ScopeContext::with_named_inputs_and_execution_context(&source, &inputs, &execution);

    assert_eq!(
        recursive_collect(
            &context,
            RecursiveCollectPaths {
                collection: &["Tree"],
                children: &["Children"],
                descent_value: &["Name"],
                values: &["Files"],
                value: &["Name"],
            },
            "",
            "/",
        ),
        Ok(vec![text("/root/top.txt"), text("/root/child/nested.txt")])
    );

    let generated = GeneratedItems::new(vec![Value::Int(1)]);
    let generated = context.generated_items(&generated);
    assert_eq!(
        generated[0].resolve_scalar(&["Tree", "Name"]),
        Ok(text("root"))
    );
    assert_eq!(
        generated[0]
            .with_compact_last_position(4)
            .resolve_scalar(&["Tree", "Name"]),
        Ok(text("root"))
    );
    assert_eq!(
        generated[0].runtime_value(RuntimeValue::MappingFilePath),
        Ok(text("map.ferrule.json"))
    );
}
