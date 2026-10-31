use super::super::schema::TUPLE_FRAME_ISSUE;
use super::*;
use crate::{
    ValidationEndpoint, ValidationIssue, ValidationOwner, ValidationSchemaLocation,
    ValidationSchemaStep,
};
use ir::{ItemCountRange, JsonTupleLayout, JsonTupleTail};

/// A closed `[string, integer]` tuple of length 1..2.
fn frame() -> SchemaNode {
    let mut frame = SchemaNode::group(
        "Point",
        vec![
            SchemaNode::scalar("item1", ScalarType::String),
            SchemaNode::scalar("item2", ScalarType::Int),
        ],
    );
    frame.json_tuple = Some(JsonTupleLayout {
        length: ItemCountRange::new(1, Some(2)),
        tail: JsonTupleTail::Closed,
    });
    frame
}

fn frame_children(frame: &mut SchemaNode) -> &mut Vec<SchemaNode> {
    let SchemaKind::Group { children, .. } = &mut frame.kind else {
        panic!("test frame must be a group");
    };
    children
}

fn project_with(node: SchemaNode) -> Project {
    let mut project = valid_project();
    let SchemaKind::Group { children, .. } = &mut project.target.kind else {
        panic!("test target must be a group");
    };
    children.push(node);
    project
}

fn tuple_issues(project: &Project) -> Vec<ValidationIssue> {
    validate(project)
        .into_iter()
        .filter(|issue| issue.message.starts_with(TUPLE_FRAME_ISSUE))
        .collect()
}

fn expected(path: &str, steps: Vec<ValidationSchemaStep>, shape: &str) -> ValidationIssue {
    ValidationIssue {
        location: "target schema".into(),
        message: format!(
            "JSON tuple frame metadata at `{path}` requires a canonical group of `item1`..`itemN` members and an optional repeating `rest` tail, none of them a JSON array: {shape}"
        ),
        owner: Some(ValidationOwner::SchemaNode(ValidationSchemaLocation {
            endpoint: ValidationEndpoint::Target,
            path: steps,
        })),
    }
}

#[test]
fn accepts_closed_open_and_repeating_tuple_frames() {
    let mut open = frame();
    frame_children(&mut open).push(SchemaNode::scalar("rest", ScalarType::Int).repeating());
    open.json_tuple = Some(JsonTupleLayout {
        length: None,
        tail: JsonTupleTail::Open,
    });
    let mut path = frame().repeating();
    path.json_object_item_nullable = Some(false);
    path.container_nullable = true;
    path.item_count_range = ItemCountRange::new(0, Some(3));
    for node in [frame(), open, path] {
        assert!(validate(&project_with(node)).is_empty());
    }
}

#[test]
fn reports_each_malformed_tuple_frame_at_its_schema_location() {
    let shape = |mutate: fn(&mut SchemaNode)| {
        let mut node = frame();
        mutate(&mut node);
        node
    };
    let cases: Vec<(&str, SchemaNode, &str)> = vec![
        (
            "misnamed member",
            shape(|frame| frame_children(frame)[1].name = "second".into()),
            "member 2 is not named `item2`",
        ),
        (
            "array member",
            shape(|frame| frame_children(frame)[0].repeating = true),
            "member 1 is a JSON array",
        ),
        (
            "closed length",
            shape(|frame| {
                frame.json_tuple = Some(JsonTupleLayout {
                    length: ItemCountRange::new(0, Some(3)),
                    tail: JsonTupleTail::Closed,
                })
            }),
            "the length bounds are not canonical for the frame's tail",
        ),
        (
            "homogeneous",
            shape(|frame| frame_children(frame)[1].kind = frame_children(frame)[0].kind.clone()),
            "the frame describes a homogeneous array",
        ),
        (
            "implicit tuple null",
            shape(|frame| frame.repeating = true),
            "the repeating frame does not declare whether each tuple may be null",
        ),
        (
            "unique items",
            shape(|frame| {
                frame.repeating = true;
                frame.json_object_item_nullable = Some(false);
                frame.json_unique_items = true;
            }),
            "the frame declares `contains` or `uniqueItems` assertions",
        ),
    ];
    for (case, node, shape) in cases {
        assert!(!node.json_tuple_frame_is_valid(), "{case}");
        assert!(
            serde_json::to_string(&node)
                .ok()
                .and_then(|text| serde_json::from_str::<SchemaNode>(&text).ok())
                .is_none(),
            "{case}"
        );
        let project = project_with(node);
        let expected = expected("Point", vec![ValidationSchemaStep::Child(1)], shape);
        assert_eq!(tuple_issues(&project), vec![expected.clone()], "{case}");
        assert_eq!(validate(&project).last(), Some(&expected), "{case}");
    }
}

#[test]
fn reports_a_frame_used_as_a_dynamic_property_value_on_its_object() {
    let mut object = SchemaNode::group("Points", Vec::new());
    let SchemaKind::Group { dynamic, .. } = &mut object.kind else {
        panic!("test object must be a group");
    };
    *dynamic = Some(Box::new(frame()));
    assert_eq!(
        tuple_issues(&project_with(object)),
        vec![expected(
            "Points",
            vec![ValidationSchemaStep::Child(1)],
            "a dynamic property value schema is a tuple frame",
        )]
    );
}
