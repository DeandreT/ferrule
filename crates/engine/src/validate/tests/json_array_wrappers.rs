use super::super::schema::WRAPPER_FRAME_ISSUE;
use super::*;
use crate::{
    ValidationEndpoint, ValidationIssue, ValidationOwner, ValidationSchemaLocation,
    ValidationSchemaStep,
};

/// The well-formed frame: an outer array of integer arrays.
fn wrapper() -> SchemaNode {
    let Some(frame) = SchemaNode::group(
        "Batches",
        vec![SchemaNode::scalar(ir::JSON_ARRAY_ITEM_FIELD, ScalarType::Int).repeating()],
    )
    .repeating()
    .with_json_array_wrapper() else {
        panic!("test frame is well formed");
    };
    frame
}

fn frame_children(frame: &mut SchemaNode) -> &mut Vec<SchemaNode> {
    let SchemaKind::Group { children, .. } = &mut frame.kind else {
        panic!("test frame must be a group");
    };
    children
}

fn project_with(frame: SchemaNode) -> Project {
    let mut project = valid_project();
    let SchemaKind::Group { children, .. } = &mut project.target.kind else {
        panic!("test target must be a group");
    };
    children.push(frame);
    project
}

fn wrapper_issues(project: &Project) -> Vec<ValidationIssue> {
    validate(project)
        .into_iter()
        .filter(|issue| issue.message.starts_with(WRAPPER_FRAME_ISSUE))
        .collect()
}

fn expected(path: &str, steps: Vec<ValidationSchemaStep>, shape: &str) -> ValidationIssue {
    ValidationIssue {
        location: "target schema".into(),
        message: format!(
            "JSON nested-array wrapper metadata at `{path}` requires a repeating group frame whose only child is a repeating `item` array: {shape}"
        ),
        owner: Some(ValidationOwner::SchemaNode(ValidationSchemaLocation {
            endpoint: ValidationEndpoint::Target,
            path: steps,
        })),
    }
}

#[test]
fn accepts_well_formed_nested_array_frames() {
    let mut nested = wrapper();
    // An outer array of arrays of integer arrays.
    frame_children(&mut nested)[0] = {
        let mut inner = wrapper();
        inner.name = ir::JSON_ARRAY_ITEM_FIELD.into();
        inner
    };
    for frame in [wrapper(), nested] {
        let mut frame = frame;
        frame.container_nullable = true;
        frame.item_count_range = ir::ItemCountRange::new(1, Some(2));
        assert!(validate(&project_with(frame)).is_empty());
    }
}

#[test]
fn reports_each_malformed_wrapper_frame_shape() {
    let shape = |mutate: fn(&mut SchemaNode)| {
        let mut frame = wrapper();
        mutate(&mut frame);
        frame
    };
    let cases: Vec<(&str, SchemaNode, &str)> = vec![
        (
            "scalar marker",
            shape(|frame| {
                frame.kind = SchemaKind::Scalar {
                    ty: ScalarType::Int,
                }
            }),
            "the frame is not a group",
        ),
        (
            "non-repeating frame",
            shape(|frame| frame.repeating = false),
            "the frame does not repeat",
        ),
        (
            "zero children",
            shape(|frame| frame_children(frame).clear()),
            "the frame has 0 children instead of one `item` child",
        ),
        (
            "two children",
            shape(|frame| {
                frame_children(frame).push(SchemaNode::scalar("extra", ScalarType::Int).repeating())
            }),
            "the frame has 2 children instead of one `item` child",
        ),
        (
            "wrong child name",
            shape(|frame| frame_children(frame)[0].name = "row".into()),
            "the frame's sole child is not named `item`",
        ),
        (
            "non-repeating child",
            shape(|frame| frame_children(frame)[0].repeating = false),
            "the frame's `item` child does not repeat",
        ),
        (
            "recursive reference",
            shape(|frame| frame.recursive_ref = Some("Loop".into())),
            "the frame declares a recursive reference",
        ),
        (
            "required property",
            shape(|frame| {
                let SchemaKind::Group { required, .. } = &mut frame.kind else {
                    panic!("test frame must be a group");
                };
                required.push(ir::JSON_ARRAY_ITEM_FIELD.into());
            }),
            "the frame declares object metadata",
        ),
        (
            "dynamic properties",
            shape(|frame| {
                let SchemaKind::Group { dynamic, .. } = &mut frame.kind else {
                    panic!("test frame must be a group");
                };
                *dynamic = Some(Box::new(SchemaNode::scalar("*", ScalarType::String)));
            }),
            "the frame declares object metadata",
        ),
        (
            "property count",
            shape(|frame| frame.property_count_range = ir::PropertyCountRange::new(1, None)),
            "the frame declares object metadata",
        ),
        (
            "object-item null",
            shape(|frame| frame.json_object_item_nullable = Some(true)),
            "the frame declares object metadata",
        ),
        (
            "scalar null",
            shape(|frame| frame.nullable = true),
            "the frame declares scalar value metadata",
        ),
        (
            "numeric range",
            shape(|frame| {
                let Some(range) = ir::IntegerRange::new(Some(0), Some(1)) else {
                    panic!("test range is valid");
                };
                frame.numeric_range = Some(ir::NumericRange::Integer(range));
            }),
            "the frame declares scalar value metadata",
        ),
        (
            "arbitrary JSON",
            shape(|frame| frame.json_any = true),
            "the frame declares scalar value metadata",
        ),
        (
            "XML attribute",
            shape(|frame| frame.attribute = true),
            "the frame declares XML metadata",
        ),
        (
            "database relation",
            shape(|frame| {
                frame.database_relation = Some(ir::DatabaseRelation {
                    parent_column: "parent_id".into(),
                    child_column: "child_id".into(),
                    foreign_key_side: ir::DatabaseForeignKeySide::Child,
                })
            }),
            "the frame declares a database relation",
        ),
    ];
    for (case, frame, shape) in cases {
        // Every refused shape is also refused by the shared reader/writer
        // invariant and by saved-project deserialization.
        assert!(!frame.json_array_wrapper_is_valid(), "{case}");
        assert!(
            serde_json::to_string(&frame)
                .ok()
                .and_then(|text| serde_json::from_str::<SchemaNode>(&text).ok())
                .is_none(),
            "{case}"
        );
        let project = project_with(frame);
        let expected = expected("Batches", vec![ValidationSchemaStep::Child(1)], shape);
        assert_eq!(wrapper_issues(&project), vec![expected.clone()], "{case}");
        // The frame issue follows every other issue its node reports.
        assert_eq!(validate(&project).last(), Some(&expected), "{case}");
    }
}

#[test]
fn reports_a_malformed_frame_nested_inside_a_well_formed_frame() {
    let mut inner = wrapper();
    inner.name = ir::JSON_ARRAY_ITEM_FIELD.into();
    frame_children(&mut inner)[0].repeating = false;
    let mut outer = wrapper();
    frame_children(&mut outer)[0] = inner;

    assert_eq!(
        wrapper_issues(&project_with(outer)),
        vec![expected(
            "Batches/item",
            vec![
                ValidationSchemaStep::Child(1),
                ValidationSchemaStep::Child(0)
            ],
            "the frame's `item` child does not repeat",
        )]
    );
}

#[test]
fn keeps_the_order_of_other_schema_issues_on_a_malformed_frame() {
    let mut frame = wrapper();
    frame.nullable = true;
    frame.json_any = true;
    let messages = validate(&project_with(frame))
        .into_iter()
        .map(|issue| issue.message)
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 3, "{messages:#?}");
    assert!(messages[0].starts_with("scalar nullability metadata at `Batches`"));
    assert!(messages[1].starts_with("arbitrary-JSON metadata at `Batches`"));
    assert!(messages[2].starts_with(WRAPPER_FRAME_ISSUE));
}
