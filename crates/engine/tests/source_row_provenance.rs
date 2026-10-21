use format_csv::{CsvByteDecodingPolicy, CsvReadOptions, CsvSourceDocument};
use ir::{Instance, ScalarType, SchemaNode};
use mapping::{Graph, Node, Project, Scope, ScopeConstruction, ScopeIteration};

#[test]
fn grouping_and_whole_row_copy_retain_each_original_physical_identity() {
    let row_schema = SchemaNode::group(
        "Rows",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    let project = Project {
        source: SchemaNode::group("Input", vec![row_schema.clone().repeating()]),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Groups",
                    vec![
                        SchemaNode::group(
                            "Member",
                            vec![SchemaNode::scalar("Value", ScalarType::String)],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [(
                1,
                Node::SourceField {
                    frame: Some(vec!["Rows".into()]),
                    path: vec!["Value".into()],
                },
            )]
            .into(),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Groups".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                group_by: Some(1),
                children: vec![Scope {
                    target_field: "Member".into(),
                    iteration: ScopeIteration::Source(vec![]),
                    construction: ScopeConstruction::CopyCurrentSource,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
    };
    assert!(engine::validate(&project).is_empty());
    let parse = || {
        CsvSourceDocument::parse_bytes(
            "same.csv",
            Some("resolved/same.csv"),
            b"Value\nA\nB\n",
            &row_schema,
            &CsvReadOptions::default(),
            CsvByteDecodingPolicy::StrictUtf8,
        )
        .unwrap()
    };
    let documents = [parse(), parse()];
    let rows: Vec<_> = documents.iter().flat_map(CsvSourceDocument::rows).collect();
    let source = Instance::Group(
        vec![(
            "Rows".into(),
            Instance::Repeated(rows.iter().map(|row| row.value().clone()).collect()),
        )]
        .into(),
    );
    let result = engine::run(&project, &source).unwrap();
    let member_a = serde_json::json!({"Group":[["Value",{"Scalar":"A"}]]});
    let member_b = serde_json::json!({"Group":[["Value",{"Scalar":"B"}]]});
    let expected = serde_json::json!({"Group":[["Groups",{"Repeated":[
        {"Group":[["Member",{"Repeated":[member_a, member_a]}]]},
        {"Group":[["Member",{"Repeated":[member_b, member_b]}]]}
    ]}]]});
    assert_eq!(serde_json::to_value(&result).unwrap(), expected);
    let Instance::Repeated(groups) = result.field("Groups").unwrap() else {
        panic!("groups")
    };
    for (group, expected_rows) in groups.iter().zip([[0, 2], [1, 3]]) {
        let Instance::Group(wrapper) = group else {
            panic!("group")
        };
        assert!(wrapper.source_origin().is_none());
        let Instance::Repeated(members) = group.field("Member").unwrap() else {
            panic!("members")
        };
        for (member, index) in members.iter().zip(expected_rows) {
            let Instance::Group(member) = member else {
                panic!("member")
            };
            let origin = member.source_origin().unwrap();
            assert!(
                origin
                    .document()
                    .same_document(rows[index].document().origin())
            );
            assert_eq!(origin.position(), rows[index].position());
        }
    }
}
