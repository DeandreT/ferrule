use super::*;
use mapping::{Binding as MappingBinding, IterationOutput as MappingIterationOutput};

fn segment(collection: &str, name: u32, branch: u32, position: u32) -> Scope {
    Scope {
        iteration: ScopeIteration::Source(vec![collection.into()]),
        iteration_output: MappingIterationOutput::Repeated,
        bindings: vec![
            MappingBinding {
                target_field: "Name".into(),
                node: name,
            },
            MappingBinding {
                target_field: "Branch".into(),
                node: branch,
            },
            MappingBinding {
                target_field: "Position".into(),
                node: position,
            },
        ],
        ..Scope::default()
    }
}

fn project(output: MappingIterationOutput) -> Project {
    let repeating = output == MappingIterationOutput::Repeated;
    let mut domestic = segment("Domestic", 1, 3, 7);
    domestic.sort_by = Some(5);
    domestic.sort_descending = true;
    domestic.windows = vec![mapping::SequenceWindow::First { count: 9 }];
    domestic.iteration_output = output;
    let mut international = segment("International", 2, 4, 8);
    international.iteration_output = output;
    let address = SchemaNode::group(
        "Address",
        vec![
            scalar("Name"),
            scalar("Branch"),
            typed_scalar("Position", ScalarType::Int),
        ],
    );
    let address = if repeating {
        address.repeating()
    } else {
        address
    };
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Domestic",
                    vec![scalar("Name"), typed_scalar("Rank", ScalarType::Int)],
                )
                .repeating(),
                SchemaNode::group(
                    "International",
                    vec![scalar("Name"), typed_scalar("Rank", ScalarType::Int)],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group("Target", vec![address]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        user_functions: BTreeMap::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::SourceField {
                        frame: Some(vec!["Domestic".into()]),
                        path: vec!["Name".into()],
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        frame: Some(vec!["International".into()]),
                        path: vec!["Name".into()],
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::String("domestic".into()),
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::String("international".into()),
                    },
                ),
                (
                    5,
                    Node::SourceField {
                        frame: Some(vec!["Domestic".into()]),
                        path: vec!["Rank".into()],
                    },
                ),
                (
                    7,
                    Node::Position {
                        collection: vec!["Domestic".into()],
                    },
                ),
                (
                    8,
                    Node::Position {
                        collection: vec!["International".into()],
                    },
                ),
                (
                    9,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Address".into(),
                iteration: ScopeIteration::Concatenate(mapping::ScopeSequence::new(
                    domestic,
                    vec![international],
                )),
                iteration_output: output,
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn lowers_ordered_repeated_scope_sequences_with_isolated_branch_controls() {
    let program =
        lower(&project(MappingIterationOutput::Repeated)).expect("repeated scope sequence lowers");
    let wrapper = &program.root.children[0];
    let iteration = wrapper.iteration.as_ref().expect("scope sequence");
    let sequence = iteration.concatenated().expect("concatenated source");
    let segments = sequence.iter().collect::<Vec<_>>();

    assert_eq!(iteration.output(), crate::IterationOutput::Repeated);
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].bindings[0].expression, 1);
    assert_eq!(
        segments[0]
            .iteration
            .as_ref()
            .map_or(0, |branch| branch.windows().len()),
        1
    );
    assert_eq!(segments[1].bindings[0].expression, 2);
}

#[test]
fn lowers_mapped_scope_sequences_without_losing_branch_output_kind() {
    let program = lower(&project(MappingIterationOutput::MappedSequence))
        .expect("mapped scope sequence lowers");
    let iteration = program.root.children[0]
        .iteration
        .as_ref()
        .expect("scope sequence");
    let sequence = iteration.concatenated().expect("concatenated source");

    assert_eq!(iteration.output(), crate::IterationOutput::MappedSequence);
    assert!(sequence.iter().all(|segment| {
        segment
            .iteration
            .as_ref()
            .is_some_and(|branch| branch.output() == crate::IterationOutput::MappedSequence)
    }));
}
