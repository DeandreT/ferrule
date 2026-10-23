//! The effective null-admission view agrees with the native JSON boundary.

use ir::{Instance, JsonNullability, ScalarType, SchemaNode, Value};

fn field(document: &Instance) -> &Instance {
    let Instance::Group(fields) = document else {
        panic!("root object: {document:?}");
    };
    fields
        .iter()
        .find_map(|(name, value)| (name == "x").then_some(value))
        .unwrap()
}

fn read(schema: &SchemaNode, text: &str) -> Option<Instance> {
    format_json::from_str(text, schema)
        .ok()
        .map(|document| field(&document).clone())
}

fn json_null() -> Instance {
    Instance::Scalar(Value::json_null())
}

fn object_array(container: bool, item: Option<bool>) -> SchemaNode {
    let mut schema = SchemaNode::group("x", vec![]).repeating();
    schema.container_nullable = container;
    schema.json_object_item_nullable = item;
    schema
}

#[test]
fn json_documents_admit_null_where_the_view_says() {
    let scalar = || SchemaNode::scalar("x", ScalarType::Int);
    let mut scalar_array_both = scalar().repeating().nullable().unwrap();
    scalar_array_both.container_nullable = true;
    let item = SchemaNode::scalar("item", ScalarType::Int)
        .repeating()
        .nullable()
        .unwrap();
    let wrapper = SchemaNode::group("x", vec![item])
        .repeating()
        .with_json_array_wrapper()
        .unwrap();
    let schemas = [
        (scalar(), "1"),
        (scalar().nullable().unwrap(), "1"),
        (SchemaNode::group("x", vec![]), "{}"),
        (
            SchemaNode::group("x", vec![]).nullable_container().unwrap(),
            "{}",
        ),
        (scalar().repeating(), "[1]"),
        (scalar().repeating().nullable().unwrap(), "[1]"),
        (scalar().repeating().nullable_container().unwrap(), "[1]"),
        (scalar_array_both, "[1]"),
        (object_array(false, None), "[{}]"),
        (object_array(true, None), "[{}]"),
        (object_array(true, Some(false)), "[{}]"),
        (object_array(false, Some(true)), "[{}]"),
        (wrapper.clone(), "[[1]]"),
        (wrapper.nullable_container().unwrap(), "[[1]]"),
    ];
    for (child, value) in schemas {
        let view = child.effective_view().unwrap();
        let root = SchemaNode::group("Root", vec![child.clone()]);
        // A present value and an absent property never read as JSON null.
        let present = read(&root, &format!("{{\"x\":{value}}}")).unwrap();
        assert_ne!(present, json_null(), "{child:?}");
        let absent = read(&root, "{}").unwrap();
        assert_ne!(absent, json_null(), "{child:?}");
        let (outer, item) = match view.nullability().json() {
            JsonNullability::Occurrence { null } => (null, None),
            JsonNullability::Array {
                container_null,
                item_null,
            } => (container_null, Some(item_null)),
            JsonNullability::NestedArrays { container_null } => (container_null, None),
        };
        assert_eq!(
            read(&root, "{\"x\":null}"),
            outer.then(json_null),
            "{child:?}"
        );
        if let Some(item) = item {
            assert_eq!(
                read(&root, "{\"x\":[null]}"),
                item.then(|| Instance::Repeated(vec![json_null()])),
                "{child:?}"
            );
        }
    }
}
